use actix_web::{test as actix_test, http::StatusCode};
use erp_cms::{serve_video_range, generate_signed_url_svod, verify_signed_url};
use std::fs::File;
use std::io::Write;

#[actix_web::test]
async fn test_serve_video_range_full() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("video_full.mp4");
    {
        let mut f = File::create(&file_path).unwrap();
        let data: Vec<u8> = (0..200).collect();
        f.write_all(&data).unwrap();
    }

    // Test request without Range header (Full delivery)
    let req = actix_test::TestRequest::get().to_http_request();
    let resp = serve_video_range(&file_path, &req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let content_len = resp.headers().get("Content-Length").unwrap().to_str().unwrap();
    assert_eq!(content_len, "200");
}

#[actix_web::test]
async fn test_serve_video_range_partial() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("video_partial.mp4");
    {
        let mut f = File::create(&file_path).unwrap();
        let data: Vec<u8> = (0..200).collect();
        f.write_all(&data).unwrap();
    }

    // Range: bytes=50-150
    let req = actix_test::TestRequest::get()
        .insert_header(("Range", "bytes=50-150"))
        .to_http_request();
    let resp = serve_video_range(&file_path, &req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
    
    let headers = resp.headers();
    let content_range = headers.get("Content-Range").unwrap().to_str().unwrap();
    assert_eq!(content_range, "bytes 50-150/200");
}

#[test]
fn test_svod_signed_urls() {
    let secret = b"my_secret_key_999";
    let base_url = "/api/stream/123.mp4";

    let signed = generate_signed_url_svod(base_url, secret, 3600);
    assert!(verify_signed_url(&signed, secret).is_ok());

    // Fails on tampered token
    let tampered = signed.replace("_token=", "_token=xyz");
    assert!(verify_signed_url(&interceptor_url_cleanup(&tampered), secret).is_err());

    // Fails on wrong secret
    assert!(verify_signed_url(&signed, b"wrong_secret").is_err());
}

fn interceptor_url_cleanup(url: &str) -> String {
    url.to_string()
}
