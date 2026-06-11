use actix_web::{
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    Error, HttpMessage,
};
use futures_util::future::LocalBoxFuture;
use std::{
    future::{ready, Ready},
};
use surrealdb::Surreal;
use surrealdb::engine::any::Any;

pub struct AppState {
    pub db: Surreal<Any>,
    pub broadcaster: tokio::sync::broadcast::Sender<String>,
}

#[derive(Clone, Debug)]
pub struct TenantContext {
    pub namespace: String,
    pub database: String,
}

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
        let host = req
            .headers()
            .get("host")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("localhost");

        let parts: Vec<&str> = host.split('.').collect();
        let db_name = if parts.len() > 1 { parts[0] } else { "default_site" };

        let tenant = TenantContext {
            namespace: "frappe_cloud".to_string(),
            database: db_name.to_string(),
        };

        req.extensions_mut().insert(tenant.clone());

        let fut = self.service.call(req);
        Box::pin(async move {
            let res = fut.await?;
            Ok(res)
        })
    }
}
