use actix_web::{
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    Error, HttpMessage, HttpResponse,
};
use futures_util::future::LocalBoxFuture;
use std::{
    collections::HashMap,
    future::{ready, Ready},
    sync::Arc,
    time::Instant,
};
use tokio::sync::RwLock;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;

/// AppState holds shared application state.
pub struct AppState {
    /// The default SurrealDB database instance.
    pub db: Surreal<Any>,
    /// The broadcaster sender for SSE.
    pub broadcaster: tokio::sync::broadcast::Sender<String>,
}

/// TenantContext holds details of the current tenant database configuration.
#[derive(Clone, Debug)]
pub struct TenantContext {
    /// The Namespace of the tenant.
    pub namespace: String,
    /// The Database name of the tenant.
    pub database: String,
}

/// TenantId is a simple wrapper around the tenant name.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TenantId(pub String);

/// TenantError represents errors during tenant connection resolution or authentication.
#[derive(Debug, thiserror::Error)]
pub enum TenantError {
    /// Connection to the database endpoint failed.
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    /// Authentication with database credentials failed.
    #[error("Authentication failed: {0}")]
    AuthFailed(String),
    /// Tenant not found in the connection records.
    #[error("Tenant not found")]
    TenantNotFound,
    /// Failed to acquire read/write lock for the connection pool.
    #[error("Lock acquisition failed")]
    LockAcquisitionFailed,
}

impl actix_web::error::ResponseError for TenantError {
    fn status_code(&self) -> actix_web::http::StatusCode {
        match self {
            TenantError::TenantNotFound => actix_web::http::StatusCode::NOT_FOUND,
            _ => actix_web::http::StatusCode::BAD_REQUEST,
        }
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code())
            .json(serde_json::json!({ "error": self.to_string() }))
    }
}

/// Thread-safe connection pool manager that maps tenant IDs to their isolated SurrealDB connections.
pub struct ConnectionPoolManager {
    /// Active connection pools.
    pub pools: Arc<RwLock<HashMap<String, Surreal<Any>>>>,
    /// Last accessed times of tenant connections.
    pub last_accessed: Arc<RwLock<HashMap<String, Instant>>>,
    /// Database endpoint (e.g. "mem://", "http://localhost:8000").
    pub db_endpoint: String,
}

impl ConnectionPoolManager {
    /// Creates a new ConnectionPoolManager with a target database endpoint.
    pub fn new(db_endpoint: String) -> Self {
        let manager = Self {
            pools: Arc::new(RwLock::new(HashMap::new())),
            last_accessed: Arc::new(RwLock::new(HashMap::new())),
            db_endpoint,
        };
        manager.start_reclamation_task();
        manager
    }

    /// Dynamically fetches or creates a connection client for the given tenant ID.
    pub async fn get_client(&self, tenant_id: &str) -> Result<Surreal<Any>, TenantError> {
        // Record last accessed time
        {
            let mut la = self.last_accessed.write().await;
            la.insert(tenant_id.to_string(), Instant::now());
        }

        // Try to retrieve connection with a read lock
        {
            let pools = self.pools.read().await;
            if let Some(client) = pools.get(tenant_id) {
                return Ok(client.clone());
            }
        }

        // Acquire write lock to initialize connection
        let mut pools = self.pools.write().await;
        if let Some(client) = pools.get(tenant_id) {
            return Ok(client.clone());
        }

        let db = surrealdb::engine::any::connect(&self.db_endpoint)
            .await
            .map_err(|e| TenantError::ConnectionFailed(e.to_string()))?;

        db.signin(surrealdb::opt::auth::Root {
            username: "root".to_string(),
            password: "root".to_string(),
        })
        .await
        .map_err(|e| TenantError::AuthFailed(e.to_string()))?;

        db.use_ns("frappe_cloud")
            .use_db(tenant_id)
            .await
            .map_err(|e| TenantError::ConnectionFailed(e.to_string()))?;

        pools.insert(tenant_id.to_string(), db.clone());
        Ok(db)
    }

    /// Spawns a background task that monitors tenant pool health and reclaims idle/unreachable connections.
    pub fn start_reclamation_task(&self) {
        let pools = Arc::clone(&self.pools);
        let last_accessed = Arc::clone(&self.last_accessed);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
            loop {
                interval.tick().await;

                let mut to_remove = Vec::new();
                {
                    let last_acc = last_accessed.read().await;
                    let now = Instant::now();
                    for (tenant_id, &last_time) in last_acc.iter() {
                        if now.duration_since(last_time) > std::time::Duration::from_secs(1800) {
                            to_remove.push(tenant_id.clone());
                        }
                    }
                }

                let keys: Vec<String> = {
                    let p = pools.read().await;
                    p.keys().cloned().collect()
                };

                for tenant_id in keys {
                    if to_remove.contains(&tenant_id) {
                        continue;
                    }
                    let client_opt = {
                        let p = pools.read().await;
                        p.get(&tenant_id).cloned()
                    };
                    if let Some(client) = client_opt {
                        if client.query("INFO FOR DB;").await.is_err() {
                            to_remove.push(tenant_id);
                        }
                    }
                }

                if !to_remove.is_empty() {
                    let mut p = pools.write().await;
                    let mut la = last_accessed.write().await;
                    for id in to_remove {
                        p.remove(&id);
                        la.remove(&id);
                    }
                }
            }
        });
    }
}

/// Global OnceLock for the connection pool manager.
pub static CONNECTION_POOL_MANAGER: std::sync::OnceLock<ConnectionPoolManager> = std::sync::OnceLock::new();

/// Resolves the global ConnectionPoolManager instance.
pub fn get_pool_manager() -> &'static ConnectionPoolManager {
    CONNECTION_POOL_MANAGER.get_or_init(|| {
        ConnectionPoolManager::new("mem://".to_string())
    })
}

/// TenantResolver intercepts HTTP requests to extract tenant ID.
pub struct TenantResolver;

impl<S, B> Transform<S, ServiceRequest> for TenantResolver
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = TenantResolverMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(TenantResolverMiddleware { service }))
    }
}

/// Middleware that extracts the tenant information and inserts it into request extensions.
pub struct TenantResolverMiddleware<S> {
    service: S,
}

impl<S, B> Service<ServiceRequest> for TenantResolverMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let mut extracted_tenant = None;

        if let Some(tenant_id_header) = req.headers().get("X-Tenant-Id") {
            if let Ok(tenant_id) = tenant_id_header.to_str() {
                extracted_tenant = Some(tenant_id.to_string());
            }
        }

        if extracted_tenant.is_none() {
            if let Some(host_header) = req.headers().get("Host") {
                if let Ok(host) = host_header.to_str() {
                    let parts: Vec<&str> = host.split('.').collect();
                    if parts.len() > 1 && parts[0] != "www" && parts[0] != "localhost" {
                        extracted_tenant = Some(parts[0].to_string());
                    }
                }
            }
        }

        let tenant_id = match extracted_tenant {
            Some(id) => id,
            None => {
                let response = HttpResponse::BadRequest()
                    .json(serde_json::json!({ "error": "Missing or invalid tenant identifier" }));
                let err = actix_web::error::InternalError::from_response(
                    "Tenant resolution failed",
                    response,
                ).into();
                return Box::pin(async move { Err(err) });
            }
        };

        // Insert TenantId and TenantContext into extensions
        let tenant_context = TenantContext {
            namespace: "frappe_cloud".to_string(),
            database: tenant_id.clone(),
        };

        req.extensions_mut().insert(TenantId(tenant_id));
        req.extensions_mut().insert(tenant_context);

        let fut = self.service.call(req);
        Box::pin(async move {
            let res = fut.await?;
            Ok(res)
        })
    }
}
