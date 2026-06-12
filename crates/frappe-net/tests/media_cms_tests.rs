use actix_web::{test, web, App};
use frappe_net::middleware::tenant::{AppState, TenantResolver};
use frappe_net::routes;
use surrealdb::engine::any::connect;
use erp_cms::security::{generate_signed_url, generate_private_embed_html};

#[actix_web::test]
async fn test_media_cms_upload_and_stream() {
    let db = connect("mem://").await.expect("Failed to connect to SurrealDB");
    let (tx, _) = tokio::sync::broadcast::channel(100);
    let app_state = web::Data::new(AppState { db: db.clone(), broadcaster: tx });

    let app = test::init_service(
        App::new()
            .app_data(app_state.clone())
            .wrap(TenantResolver)
            .configure(routes::config)
    ).await;

    // 1. Prepare dummy file content (500 bytes of sequential numbers)
    let mut dummy_video_data = Vec::new();
    for i in 0..500 {
        dummy_video_data.push((i % 256) as u8);
    }

    // 2. Build raw multipart request payload for basic upload
    let boundary = "AaB03x";
    let mut payload = Vec::new();
    
    // Part 1: tenant_id
    payload.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    payload.extend_from_slice(b"Content-Disposition: form-data; name=\"tenant_id\"\r\n\r\n");
    payload.extend_from_slice(b"tenant1\r\n");

    // Part 2: file
    payload.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    payload.extend_from_slice(b"Content-Disposition: form-data; name=\"file\"; filename=\"test_video.mp4\"\r\n");
    payload.extend_from_slice(b"Content-Type: video/mp4\r\n\r\n");
    payload.extend_from_slice(&dummy_video_data);
    payload.extend_from_slice(b"\r\n");

    // Final boundary
    payload.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());

    // 3. Send upload request
    let req_upload = test::TestRequest::post()
        .uri("/api/storage/upload")
        .insert_header(("Host", "tenant1.localhost"))
        .insert_header(("Content-Type", format!("multipart/form-data; boundary={}", boundary)))
        .set_payload(payload)
        .to_request();

    let resp_upload = test::call_service(&app, req_upload).await;
    assert!(resp_upload.status().is_success());

    let body_upload: serde_json::Value = test::read_body_json(resp_upload).await;
    let file_hash = body_upload["file_hash"].as_str().expect("Missing file_hash in upload response").to_string();
    assert!(!file_hash.is_empty());

    // Check if file actually exists in storage
    let file_path = std::path::PathBuf::from("./data/storage").join("tenant1").join(&file_hash);
    assert!(file_path.exists());

    // 4. Check initial storage metrics
    let req_metrics1 = test::TestRequest::get()
        .uri("/api/storage/metrics")
        .insert_header(("Host", "tenant1.localhost"))
        .to_request();
    let resp_metrics1 = test::call_service(&app, req_metrics1).await;
    assert!(resp_metrics1.status().is_success());
    let metrics1: serde_json::Value = test::read_body_json(resp_metrics1).await;
    assert_eq!(metrics1["total_files"].as_u64().unwrap_or(0), 1);
    assert_eq!(metrics1["total_size_bytes"].as_u64().unwrap_or(0), 500);
    assert_eq!(metrics1["total_transcoded_files"].as_u64().unwrap_or(0), 0);

    // 5. Build raw multipart request payload for automatic transcoding
    let mut dummy_video_data2 = Vec::new();
    for i in 0..500 {
        dummy_video_data2.push(((i + 13) % 256) as u8);
    }

    let mut payload_trans = Vec::new();
    payload_trans.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    payload_trans.extend_from_slice(b"Content-Disposition: form-data; name=\"tenant_id\"\r\n\r\n");
    payload_trans.extend_from_slice(b"tenant1\r\n");
    payload_trans.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    payload_trans.extend_from_slice(b"Content-Disposition: form-data; name=\"file\"; filename=\"test_video.raw\"\r\n");
    payload_trans.extend_from_slice(b"Content-Type: video/raw\r\n\r\n");
    payload_trans.extend_from_slice(&dummy_video_data2);
    payload_trans.extend_from_slice(b"\r\n");
    payload_trans.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());

    let req_upload_trans = test::TestRequest::post()
        .uri("/api/storage/upload?transcode=true")
        .insert_header(("Host", "tenant1.localhost"))
        .insert_header(("Content-Type", format!("multipart/form-data; boundary={}", boundary)))
        .set_payload(payload_trans)
        .to_request();

    let resp_upload_trans = test::call_service(&app, req_upload_trans).await;
    assert!(resp_upload_trans.status().is_success());
    let body_upload_trans: serde_json::Value = test::read_body_json(resp_upload_trans).await;
    let raw_hash = body_upload_trans["file_hash"].as_str().expect("Missing raw hash");
    let transcoded_hash = body_upload_trans["transcoded_hash"].as_str().expect("Missing transcoded_hash");
    assert_ne!(raw_hash, transcoded_hash);

    // Check that both files exist in storage
    assert!(std::path::PathBuf::from("./data/storage").join("tenant1").join(raw_hash).exists());
    assert!(std::path::PathBuf::from("./data/storage").join("tenant1").join(transcoded_hash).exists());

    // 6. Check updated storage metrics showing relation and counts
    let req_metrics2 = test::TestRequest::get()
        .uri("/api/storage/metrics")
        .insert_header(("Host", "tenant1.localhost"))
        .to_request();
    let resp_metrics2 = test::call_service(&app, req_metrics2).await;
    assert!(resp_metrics2.status().is_success());
    let metrics2: serde_json::Value = test::read_body_json(resp_metrics2).await;
    assert_eq!(metrics2["total_files"].as_u64().unwrap_or(0), 3); // 1st file + 2nd raw file + 2nd transcoded file
    assert_eq!(metrics2["total_transcoded_files"].as_u64().unwrap_or(0), 1);

    // 7. Generate signed URL for streaming
    let secret = b"my_secret_key";
    let base_url = format!("/api/cms/video/stream/{}", file_hash);
    let signed_url = generate_signed_url(&base_url, &file_hash, secret, 3600);

    // 8. Test access denied (no signature)
    let req_unsigned = test::TestRequest::get()
        .uri(&base_url)
        .insert_header(("Host", "tenant1.localhost"))
        .to_request();
    let resp_unsigned = test::call_service(&app, req_unsigned).await;
    assert_eq!(resp_unsigned.status(), actix_web::http::StatusCode::FORBIDDEN);

    // 9. Test valid signed URL - Full stream request (no Range header)
    let req_stream_full = test::TestRequest::get()
        .uri(&signed_url)
        .insert_header(("Host", "tenant1.localhost"))
        .to_request();
    let resp_stream_full = test::call_service(&app, req_stream_full).await;
    assert!(resp_stream_full.status().is_success());
    let body_full = test::read_body(resp_stream_full).await;
    assert_eq!(body_full.len(), 500);
    assert_eq!(body_full.to_vec(), dummy_video_data);

    // 10. Test valid signed URL - Byte-range request (bytes=100-200)
    let req_stream_range = test::TestRequest::get()
        .uri(&signed_url)
        .insert_header(("Host", "tenant1.localhost"))
        .insert_header(("Range", "bytes=100-200"))
        .to_request();
    let resp_stream_range = test::call_service(&app, req_stream_range).await;
    assert_eq!(resp_stream_range.status(), actix_web::http::StatusCode::PARTIAL_CONTENT);

    // Verify content-range header
    let content_range = resp_stream_range.headers().get("Content-Range")
        .and_then(|h| h.to_str().ok())
        .expect("Missing Content-Range header");
    assert_eq!(content_range, "bytes 100-200/500");

    let body_range = test::read_body(resp_stream_range).await;
    assert_eq!(body_range.len(), 101);
    assert_eq!(body_range.to_vec(), dummy_video_data[100..=200].to_vec());

    // 11. Verify generate_private_embed_html outputs valid tag and signature url
    let embed_options = erp_cms::EmbedOptions {
        autoplay: true,
        controls: true,
        loop_video: false,
        muted: true,
        poster_url: None,
    };
    let embed_html = generate_private_embed_html(
        "/api/cms/video/stream/video123",
        "video123",
        secret,
        1800,
        &embed_options,
    );
    assert!(embed_html.contains("<video"));
    assert!(embed_html.contains("src=\"/api/cms/video/stream/video123?_token="));
    assert!(embed_html.contains("_hash=video123"));

    // Clean up temporary storage directory
    let _ = std::fs::remove_dir_all("./data/storage/tenant1");
}
