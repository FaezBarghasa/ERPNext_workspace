use actix_web::{get, web, HttpResponse, Responder};
use serde_json::json;
use crate::middleware::tenant::{AppState, TenantContext};
use crate::sse::sse_events;
use rust_decimal::Decimal;
use serde::Deserialize;

#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub struct GenericDocument {
    pub id: serde_json::Value,
    #[serde(flatten)]
    pub fields: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Deserialize, Debug)]
pub struct ClockInRequest {
    pub employee_id: String,
    pub lat: Decimal,
    pub lon: Decimal,
    pub factory_lat: Decimal,
    pub factory_lon: Decimal,
    pub threshold_meters: Decimal,
}

#[derive(Deserialize, Debug)]
pub struct VideoEmbedRequest {
    pub url: String,
    pub autoplay: Option<bool>,
    pub controls: Option<bool>,
    pub loop_video: Option<bool>,
    pub muted: Option<bool>,
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
            let docs_vals: Result<Vec<serde_json::Value>, surrealdb::Error> = res.take(1);
            let docs: Vec<GenericDocument> = docs_vals.unwrap_or_default().into_iter()
                .map(|v| serde_json::from_value(v).unwrap())
                .collect();
            if let Some(d) = docs.into_iter().next() {
                HttpResponse::Ok().json(json!({ "data": d }))
            } else {
                HttpResponse::NotFound().json(json!({ "error": format!("Document {} not found", name) }))
            }
        }
        Err(e) => HttpResponse::InternalServerError().json(json!({ "error": e.to_string() })),
    }
}

#[actix_web::post("/api/worker/clock-in")]
pub async fn worker_clock_in(
    payload: web::Json<ClockInRequest>,
    state: web::Data<AppState>,
    tenant: web::ReqData<TenantContext>,
) -> impl Responder {
    let _ = state.db.use_ns(&tenant.namespace).use_db(&tenant.database).await;

    match erp_hr::attendance::record_factory_clock_in(
        &state.db,
        payload.employee_id.clone(),
        payload.lat,
        payload.lon,
        payload.factory_lat,
        payload.factory_lon,
        payload.threshold_meters,
    ).await {
        Ok(success) => {
            if success {
                HttpResponse::Ok().json(json!({
                    "status": "success",
                    "message": "Clock-in recorded successfully"
                }))
            } else {
                HttpResponse::BadRequest().json(json!({
                    "status": "failed",
                    "error": "Geofence validation failed. Worker is outside the factory boundary."
                }))
            }
        }
        Err(e) => HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "error": e
        }))
    }
}

#[actix_web::get("/api/worker/dashboard-metrics")]
pub async fn worker_dashboard_metrics(
    query: web::Query<std::collections::HashMap<String, String>>,
    state: web::Data<AppState>,
    tenant: web::ReqData<TenantContext>,
) -> impl Responder {
    let employee_id = match query.get("employee_id") {
        Some(id) => id.clone(),
        None => return HttpResponse::BadRequest().json(json!({ "error": "employee_id parameter is required" })),
    };

    let _ = state.db.use_ns(&tenant.namespace).use_db(&tenant.database).await;

    let db_query = "SELECT * FROM tabAttendance WHERE employee_id = $emp ORDER BY date DESC LIMIT 5;";
    let mut db_res = match state.db.query(db_query).bind(("emp", employee_id.clone())).await {
        Ok(r) => r,
        Err(e) => return HttpResponse::InternalServerError().json(json!({ "error": e.to_string() })),
    };

    let records_vals: Vec<serde_json::Value> = db_res.take(0).unwrap_or_default();
    let records: Vec<erp_hr::attendance::AttendanceRecord> = records_vals.into_iter()
        .map(|v| serde_json::from_value(v).unwrap())
        .collect();

    let logs_query = "SELECT * FROM tabBiometricLog WHERE employee_id = $emp ORDER BY timestamp DESC LIMIT 5;";
    let mut logs_res = match state.db.query(logs_query).bind(("emp", employee_id.clone())).await {
        Ok(r) => r,
        Err(e) => return HttpResponse::InternalServerError().json(json!({ "error": e.to_string() })),
    };

    let logs: Vec<serde_json::Value> = logs_res.take(0).unwrap_or_default();

    let current_record = records.first();
    let clock_in_time = current_record.map(|r| r.clock_in);
    let clock_out_time = current_record.and_then(|r| r.clock_out);
    let attendance_status = current_record.map(|r| r.status.as_str()).unwrap_or("Absent");
    
    let shift_status = if current_record.is_some() && clock_out_time.is_none() {
        "Active"
    } else {
        "Inactive"
    };

    HttpResponse::Ok().json(json!({
        "employee_id": employee_id,
        "attendance_status": attendance_status,
        "clock_in_time": clock_in_time,
        "clock_out_time": clock_out_time,
        "shift_status": shift_status,
        "recent_attendance": records,
        "recent_biometric_logs": logs,
        "productivity_score": 92.5
    }))
}

#[actix_web::get("/api/cms/video-embed")]
pub async fn get_video_embed(
    query: web::Query<VideoEmbedRequest>,
) -> impl Responder {
    match erp_cms::video_player::parse_video_url(&query.url) {
        Ok(info) => {
            let options = erp_cms::video_player::EmbedOptions {
                autoplay: query.autoplay.unwrap_or(false),
                controls: query.controls.unwrap_or(true),
                loop_video: query.loop_video.unwrap_or(false),
                muted: query.muted.unwrap_or(false),
                poster_url: None,
            };
            let html = erp_cms::video_player::generate_embed_html(&info, &options);
            HttpResponse::Ok().json(json!({
                "platform": format!("{:?}", info.platform),
                "video_id": info.video_id,
                "html": html
            }))
        }
        Err(e) => HttpResponse::BadRequest().json(json!({
            "error": e.to_string()
        }))
    }
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(get_document)
       .service(sse_events)
       .service(worker_clock_in)
       .service(worker_dashboard_metrics)
       .service(get_video_embed);
}
