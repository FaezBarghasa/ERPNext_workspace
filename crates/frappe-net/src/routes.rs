use actix_web::{get, web, HttpResponse, Responder};
use serde_json::json;
use crate::middleware::tenant::{AppState, TenantContext};
use crate::sse::sse_events;

#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub struct GenericDocument {
    pub id: surrealdb::sql::Thing,
    #[serde(flatten)]
    pub fields: std::collections::HashMap<String, serde_json::Value>,
}

#[get("/api/resource/{doctype}/{name}")]
pub async fn get_document(
    path: web::Path<(String, String)>,
    state: web::Data<AppState>,
    tenant: web::ReqData<TenantContext>,
) -> impl Responder {
    let (doctype, name) = path.into_inner();

    // Switch to tenant DB context (fallback/session initialization)
    let _ = state.db.use_ns(&tenant.namespace).use_db(&tenant.database).await;

    // Run dynamic selection query prefixed with USE statement to guarantee isolation
    let query = format!("USE NS {} DB {}; SELECT * FROM {} WHERE name = $name;", tenant.namespace, tenant.database, doctype);
    match state.db.query(query).bind(("name", name.clone())).await {
        Ok(mut res) => {
            // Index 0 is the USE statement. Index 1 is the SELECT statement.
            let docs_res: Result<Vec<GenericDocument>, surrealdb::Error> = res.take(1);
            let docs = docs_res.unwrap_or_default();
            if let Some(d) = docs.into_iter().next() {
                HttpResponse::Ok().json(json!({ "data": d }))
            } else {
                HttpResponse::NotFound().json(json!({ "error": format!("Document {} not found", name) }))
            }
        }
        Err(e) => HttpResponse::InternalServerError().json(json!({ "error": e.to_string() })),
    }
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(get_document)
       .service(sse_events);
}
