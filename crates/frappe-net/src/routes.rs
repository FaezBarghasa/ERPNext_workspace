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

#[derive(Deserialize, Debug)]
pub struct UploadQuery {
    pub transcode: Option<bool>,
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
       .service(get_video_embed)
       .service(upload_file)
       .service(stream_video)
       .service(storage_metrics);
}

pub struct MultipartField {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub data: Vec<u8>,
}

fn extract_boundary(content_type: &str) -> Option<String> {
    let pattern = "boundary=";
    if let Some(pos) = content_type.find(pattern) {
        let start = pos + pattern.len();
        let remainder = &content_type[start..];
        let remainder = remainder.trim();
        if let Some(stripped) = remainder.strip_prefix('"') {
            if let Some(end) = stripped.find('"') {
                return Some(stripped[..end].to_string());
            }
        } else {
            let end = remainder.find(|c: char| c == ';' || c.is_whitespace()).unwrap_or(remainder.len());
            return Some(remainder[..end].to_string());
        }
    }
    None
}

fn find_subslice(haystack: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() || start > haystack.len() - needle.len() {
        return None;
    }
    for i in start..=(haystack.len() - needle.len()) {
        if &haystack[i..i + needle.len()] == needle {
            return Some(i);
        }
    }
    None
}

fn parse_multipart(body: &[u8], boundary: &str) -> Vec<MultipartField> {
    let delimiter = format!("--{}", boundary);
    let delimiter_bytes = delimiter.as_bytes();
    let boundary_len = delimiter_bytes.len();
    
    let mut fields = Vec::new();
    let mut search_start = 0;
    
    while let Some(part_start) = find_subslice(body, delimiter_bytes, search_start) {
        let content_start = part_start + boundary_len;
        if content_start >= body.len() {
            break;
        }
        
        if body[content_start..].starts_with(b"--") {
            break;
        }
        
        let part_end = find_subslice(body, delimiter_bytes, content_start).unwrap_or(body.len());
        let part_content = &body[content_start..part_end];
        
        let header_end_pattern = b"\r\n\r\n";
        let opt_header_end = find_subslice(part_content, header_end_pattern, 0);
        
        if let Some(h_end) = opt_header_end {
            let header_bytes = &part_content[..h_end];
            let body_bytes = &part_content[h_end + header_end_pattern.len()..];
            
            let header_str = String::from_utf8_lossy(header_bytes);
            let mut name = String::new();
            let mut filename = None;
            let mut content_type = None;
            
            for line in header_str.lines() {
                let line_lower = line.to_lowercase();
                if line_lower.starts_with("content-disposition:") {
                    let disp_pattern = "name=";
                    if let Some(pos) = line.find(disp_pattern) {
                        let start = pos + disp_pattern.len();
                        let rem = &line[start..];
                        let term = rem.find(|c: char| c == ';' || c == '\r' || c == '\n').unwrap_or(rem.len());
                        name = rem[..term].trim().trim_matches('"').to_string();
                    }
                    
                    let file_pattern = "filename=";
                    if let Some(pos) = line.find(file_pattern) {
                        let start = pos + file_pattern.len();
                        let rem = &line[start..];
                        let term = rem.find(|c: char| c == ';' || c == '\r' || c == '\n').unwrap_or(rem.len());
                        filename = Some(rem[..term].trim().trim_matches('"').to_string());
                    }
                } else if line_lower.starts_with("content-type:") {
                    let parts: Vec<&str> = line.splitn(2, ':').collect();
                    if parts.len() > 1 {
                        content_type = Some(parts[1].trim().to_string());
                    }
                }
            }
            
            let body_len = body_bytes.len();
            let trimmed_body = if body_len >= 2 && &body_bytes[body_len - 2..] == b"\r\n" {
                &body_bytes[..body_len - 2]
            } else if body_len >= 1 && body_bytes[body_len - 1] == b'\n' {
                &body_bytes[..body_len - 1]
            } else {
                body_bytes
            };
            
            fields.push(MultipartField {
                name,
                filename,
                content_type,
                data: trimmed_body.to_vec(),
            });
        }
        
        search_start = part_end;
    }
    fields
}

#[actix_web::post("/api/storage/upload")]
pub async fn upload_file(
    payload: actix_web::web::Bytes,
    req: actix_web::HttpRequest,
    query: web::Query<UploadQuery>,
    tenant: web::ReqData<TenantContext>,
    state: web::Data<AppState>,
) -> impl Responder {
    let content_type = req.headers().get("content-type")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
        
    let boundary = match extract_boundary(content_type) {
        Some(b) => b,
        None => return HttpResponse::BadRequest().json(json!({ "error": "Missing or invalid boundary in Content-Type" })),
    };
    
    let fields = parse_multipart(&payload, &boundary);
    let mut file_field = None;
    let mut tenant_id_field = None;
    
    for field in fields {
        if field.name == "file" {
            file_field = Some(field);
        } else if field.name == "tenant_id" {
            tenant_id_field = Some(String::from_utf8_lossy(&field.data).to_string());
        }
    }
    
    let file = match file_field {
        Some(f) => f,
        None => return HttpResponse::BadRequest().json(json!({ "error": "Missing file field" })),
    };
    
    let tenant_id = tenant_id_field.unwrap_or_else(|| tenant.database.clone());
    let original_name = file.filename.unwrap_or_else(|| "upload.bin".to_string());
    
    let storage_root = "./data/storage";
    let file_bytes = file.data;
    let size_bytes = file_bytes.len() as u64;
    
    let store_stream = futures_util::stream::once(std::future::ready(Ok::<_, std::io::Error>(file_bytes)));
    match frappe_storage::local_fs::store_file_stream(store_stream, &tenant_id, storage_root).await {
        Ok(hash) => {
            let url = format!("/api/storage/file/{}", hash);
            
            // Switch to tenant DB context to log metadata
            let _ = state.db.use_ns(&tenant.namespace).use_db(&tenant.database).await;
            
            // Insert metadata record in SurrealDB
            let db_query = format!(
                "CREATE File:{} SET file_hash = $hash, original_name = $name, size_bytes = $size, url = $url, uploaded_at = time::now();",
                hash
            );
            let _ = state.db.query(db_query)
                .bind(("hash", hash.clone()))
                .bind(("name", original_name.clone()))
                .bind(("size", size_bytes))
                .bind(("url", url.clone()))
                .await;
                
            let mut transcoded_hash = None;
            let name_lower = original_name.to_lowercase();
            let is_video = name_lower.ends_with(".mov")
                || name_lower.ends_with(".avi")
                || name_lower.ends_with(".mkv")
                || name_lower.ends_with(".raw")
                || name_lower.ends_with(".mp4");
                
            if is_video && query.transcode.unwrap_or(false) {
                let input_path = std::path::PathBuf::from(storage_root).join(&tenant_id).join(&hash);
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                let temp_output_name = format!("trans_{}_{}.mp4", hash, nanos);
                let temp_output_path = std::path::PathBuf::from(storage_root).join(&tenant_id).join(&temp_output_name);
                
                match erp_cms::transcoder::VideoTranscoder::transcode(&input_path, &temp_output_path, erp_cms::transcoder::VideoFormat::Mp4).await {
                    Ok(_) => {
                        if let Ok(trans_bytes) = tokio::fs::read(&temp_output_path).await {
                            let trans_size = trans_bytes.len() as u64;
                            let digest = ring::digest::digest(&ring::digest::SHA256, &trans_bytes);
                            let t_hash: String = digest.as_ref().iter().map(|b| format!("{:02x}", b)).collect();
                            
                            let final_transcoded_path = std::path::PathBuf::from(storage_root).join(&tenant_id).join(&t_hash);
                            if final_transcoded_path.exists() {
                                let _ = tokio::fs::remove_file(&temp_output_path).await;
                            } else {
                                let _ = tokio::fs::rename(&temp_output_path, &final_transcoded_path).await;
                            }
                            
                            // Register transcoded file in SurrealDB
                            let url_transcoded = format!("/api/storage/file/{}", t_hash);
                            let name_transcoded = format!("transcoded_{}", original_name);
                            let query_trans = format!(
                                "CREATE File:{} SET file_hash = $hash, original_name = $name, size_bytes = $size, url = $url, uploaded_at = time::now();",
                                t_hash
                            );
                            let _ = state.db.query(query_trans)
                                .bind(("hash", t_hash.clone()))
                                .bind(("name", name_transcoded))
                                .bind(("size", trans_size))
                                .bind(("url", url_transcoded))
                                .await;
                                
                            // Relate raw file to transcoded file in SurrealDB
                            let relate_query = format!(
                                "RELATE File:{}->transcoded_to->File:{} CONTENT {{ format: 'mp4' }};",
                                hash, t_hash
                            );
                            let _ = state.db.query(relate_query).await;
                            transcoded_hash = Some(t_hash);
                        }
                    }
                    Err(e) => {
                        log::error!("Auto-transcoding failed: {:?}", e);
                    }
                }
            }
            
            HttpResponse::Ok().json(json!({
                "file_hash": hash,
                "url": url,
                "original_name": original_name,
                "size_bytes": size_bytes,
                "transcoded_hash": transcoded_hash,
            }))
        }
        Err(e) => HttpResponse::InternalServerError().json(json!({ "error": format!("Failed to store file: {}", e) })),
    }
}

#[actix_web::get("/api/cms/video/stream/{hash}")]
pub async fn stream_video(
    path: web::Path<String>,
    req: actix_web::HttpRequest,
    tenant: web::ReqData<TenantContext>,
) -> impl Responder {
    let hash = path.into_inner();
    
    let request_uri = format!("{}?{}", req.path(), req.query_string());
    let secret = b"my_secret_key";
    if let Err(e) = erp_cms::security::validate_signed_url(&request_uri, secret) {
        return HttpResponse::Forbidden().body(format!("Access Denied: {}", e));
    }
    
    let tenant_id = tenant.database.clone();
    let file_path = std::path::PathBuf::from("./data/storage").join(&tenant_id).join(&hash);
    
    if !file_path.exists() {
        return HttpResponse::NotFound().body("Video file not found");
    }
    
    let file_len = match std::fs::metadata(&file_path) {
        Ok(m) => m.len(),
        Err(e) => return HttpResponse::InternalServerError().body(format!("IO error: {}", e)),
    };
    
    let mut start = 0;
    let mut end = file_len - 1;
    let mut is_range = false;
    
    if let Some(range_header) = req.headers().get("Range").and_then(|r| r.to_str().ok()) {
        if range_header.starts_with("bytes=") {
            let range_str = &range_header["bytes=".len()..];
            let parts: Vec<&str> = range_str.split('-').collect();
            if !parts.is_empty() {
                if let Ok(s) = parts[0].parse::<u64>() {
                    start = s;
                    is_range = true;
                }
                if parts.len() > 1 && !parts[1].is_empty() {
                    if let Ok(e_val) = parts[1].parse::<u64>() {
                        end = std::cmp::min(e_val, file_len - 1);
                        is_range = true;
                    }
                }
            }
        }
    }
    
    if start > end || start >= file_len {
        return HttpResponse::RangeNotSatisfiable()
            .insert_header(("Content-Range", format!("bytes */{}", file_len)))
            .finish();
    }
    
    use std::io::{Read, Seek, SeekFrom};
    let mut file = match std::fs::File::open(&file_path) {
        Ok(f) => f,
        Err(e) => return HttpResponse::InternalServerError().body(format!("IO error: {}", e)),
    };
    
    if let Err(e) = file.seek(SeekFrom::Start(start)) {
        return HttpResponse::InternalServerError().body(format!("Seek error: {}", e));
    }
    
    let chunk_len = end - start + 1;
    let mut buffer = vec![0; chunk_len as usize];
    if let Err(e) = file.read_exact(&mut buffer) {
        return HttpResponse::InternalServerError().body(format!("Read error: {}", e));
    }
    
    if is_range {
        HttpResponse::PartialContent()
            .insert_header(("Content-Range", format!("bytes {}-{}/{}", start, end, file_len)))
            .insert_header(("Content-Type", "video/mp4"))
            .body(buffer)
    } else {
        HttpResponse::Ok()
            .insert_header(("Content-Length", file_len.to_string()))
            .insert_header(("Content-Type", "video/mp4"))
            .body(buffer)
    }
}

#[actix_web::get("/api/storage/metrics")]
pub async fn storage_metrics(
    tenant: web::ReqData<TenantContext>,
    state: web::Data<AppState>,
) -> impl Responder {
    let _ = state.db.use_ns(&tenant.namespace).use_db(&tenant.database).await;
    
    // Query file counts and total sizes
    let stats_res = state.db.query("SELECT count() AS total_count, math::sum(size_bytes) AS total_bytes FROM File GROUP ALL").await;
    let mut total_count = 0;
    let mut total_bytes = 0;
    if let Ok(mut res) = stats_res {
        if let Ok(Some(row)) = res.take::<Option<serde_json::Value>>(0) {
            total_count = row["total_count"].as_u64().unwrap_or(0);
            total_bytes = row["total_bytes"].as_u64().unwrap_or(0);
        }
    }
    
    // Query count of transcoded relations
    let relations_res = state.db.query("SELECT count() AS count FROM transcoded_to GROUP ALL").await;
    let mut transcoded_count = 0;
    if let Ok(mut res) = relations_res {
        if let Ok(Some(row)) = res.take::<Option<serde_json::Value>>(0) {
            transcoded_count = row["count"].as_u64().unwrap_or(0);
        }
    }
    
    // Query recent uploads
    let recent_res = state.db.query("SELECT file_hash, original_name, size_bytes, url FROM File ORDER BY uploaded_at DESC LIMIT 5").await;
    let mut recent_uploads = Vec::new();
    if let Ok(mut res) = recent_res {
        if let Ok(rows) = res.take::<Vec<serde_json::Value>>(0) {
            recent_uploads = rows;
        }
    }
    
    HttpResponse::Ok().json(json!({
        "total_files": total_count,
        "total_size_bytes": total_bytes,
        "total_transcoded_files": transcoded_count,
        "recent_uploads": recent_uploads,
    }))
}
