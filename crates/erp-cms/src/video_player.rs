use serde::{Deserialize, Serialize};
use regex::Regex;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use hmac::{Hmac, Mac};
use std::io::Write;

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum VideoPlatform {
    YouTube,
    Vimeo,
    Html5,
    S3Custom,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VideoInfo {
    pub platform: VideoPlatform,
    pub video_id: String, // ID or direct video stream URL
    pub original_url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct EmbedOptions {
    pub autoplay: bool,
    pub controls: bool,
    pub loop_video: bool,
    pub muted: bool,
    pub poster_url: Option<String>,
}

#[derive(thiserror::Error, Debug)]
pub enum VideoError {
    #[error("Unsupported video platform or invalid URL format: {0}")]
    UnsupportedUrl(String),
}

pub fn parse_video_url(url: &str) -> Result<VideoInfo, VideoError> {
    let url_trimmed = url.trim();

    // YouTube regex patterns
    let yt_reg = Regex::new(r#"(?:youtube\.com/(?:[^/]+/.+/|(?:v|e(?:mbed)?)/|.*[?&]v=)|youtu\.be/)([^"&?/\s]{11})"#).unwrap();
    if let Some(caps) = yt_reg.captures(url_trimmed) {
        if let Some(id) = caps.get(1) {
            return Ok(VideoInfo {
                platform: VideoPlatform::YouTube,
                video_id: id.as_str().to_string(),
                original_url: url_trimmed.to_string(),
            });
        }
    }

    // Vimeo regex patterns
    let vimeo_reg = Regex::new(r#"vimeo\.com/(?:video/)?([0-9]+)"#).unwrap();
    if let Some(caps) = vimeo_reg.captures(url_trimmed) {
        if let Some(id) = caps.get(1) {
            return Ok(VideoInfo {
                platform: VideoPlatform::Vimeo,
                video_id: id.as_str().to_string(),
                original_url: url_trimmed.to_string(),
            });
        }
    }

    // S3 Custom URLs (e.g. *.s3.amazonaws.com/*.mp4)
    if url_trimmed.contains(".s3.") && (url_trimmed.ends_with(".mp4") || url_trimmed.ends_with(".webm") || url_trimmed.ends_with(".m3u8")) {
        return Ok(VideoInfo {
            platform: VideoPlatform::S3Custom,
            video_id: url_trimmed.to_string(),
            original_url: url_trimmed.to_string(),
        });
    }

    // Direct HTML5 video extensions
    if url_trimmed.ends_with(".mp4") || url_trimmed.ends_with(".webm") || url_trimmed.ends_with(".ogg") {
        return Ok(VideoInfo {
            platform: VideoPlatform::Html5,
            video_id: url_trimmed.to_string(),
            original_url: url_trimmed.to_string(),
        });
    }

    Err(VideoError::UnsupportedUrl(url_trimmed.to_string()))
}

pub fn generate_embed_html(info: &VideoInfo, options: &EmbedOptions) -> String {
    let mut params = Vec::new();

    match info.platform {
        VideoPlatform::YouTube => {
            if options.autoplay {
                params.push("autoplay=1".to_string());
                params.push("mute=1".to_string());
            }
            if !options.controls {
                params.push("controls=0".to_string());
            }
            if options.loop_video {
                params.push("loop=1".to_string());
                params.push(format!("playlist={}", info.video_id));
            }
            let query = if params.is_empty() {
                "".to_string()
            } else {
                format!("?{}", params.join("&"))
            };
            format!(
                r#"<div class="video-container" style="position:relative;padding-bottom:56.25%;height:0;overflow:hidden;max-width:100%;"><iframe src="https://www.youtube.com/embed/{}{}" frameborder="0" allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture" allowfullscreen style="position:absolute;top:0;left:0;width:100%;height:100%;"></iframe></div>"#,
                info.video_id, query
            )
        }
        VideoPlatform::Vimeo => {
            if options.autoplay {
                params.push("autoplay=1".to_string());
                params.push("muted=1".to_string());
            }
            if !options.controls {
                params.push("controls=0".to_string());
            }
            if options.loop_video {
                params.push("loop=1".to_string());
            }
            let query = if params.is_empty() {
                "".to_string()
            } else {
                format!("?{}", params.join("&"))
            };
            format!(
                r#"<div class="video-container" style="position:relative;padding-bottom:56.25%;height:0;overflow:hidden;max-width:100%;"><iframe src="https://player.vimeo.com/video/{}{}" frameborder="0" allow="autoplay; fullscreen; picture-in-picture" allowfullscreen style="position:absolute;top:0;left:0;width:100%;height:100%;"></iframe></div>"#,
                info.video_id, query
            )
        }
        VideoPlatform::Html5 | VideoPlatform::S3Custom => {
            let mut attrs = Vec::new();
            attrs.push("playsinline".to_string());
            if options.autoplay {
                attrs.push("autoplay".to_string());
                attrs.push("muted".to_string());
            }
            if options.controls {
                attrs.push("controls".to_string());
            }
            if options.loop_video {
                attrs.push("loop".to_string());
            }
            if options.muted && !options.autoplay {
                attrs.push("muted".to_string());
            }
            if let Some(ref poster) = options.poster_url {
                attrs.push(format!(r#"poster="{}""#, poster));
            }
            let content_type = if info.video_id.ends_with(".webm") {
                "video/webm"
            } else {
                "video/mp4"
            };
            format!(
                r#"<div class="video-container" style="width:100%;max-width:100%;overflow:hidden;"><video {} style="width:100%;height:auto;display:block;"><source src="{}" type="{}">Your browser does not support the video tag.</video></div>"#,
                attrs.join(" "),
                info.video_id,
                content_type
            )
        }
    }
}

// ── SVoD Ingestion, Transcoding, Vector Search, and DRM Range Responders ──────

/// Intercepts incoming byte stream from Multipart to support chunked multi-gigabyte uploads
/// with a maximum 8MB buffer size limit on RAM accumulation. Writes to hash-addressed output file.
pub async fn handle_multipart_upload(
    mut payload: actix_multipart::Multipart,
    target_dir: &str,
) -> Result<String, actix_web::Error> {
    let mut file_hash = String::new();
    
    while let Some(item) = payload.next().await {
        let mut field = item?;
        let content_disposition = field.content_disposition();
        
        if content_disposition.get_filename().is_some() {
            let temp_name = format!(
                "upload_{}.tmp",
                chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
            );
            let temp_path = std::path::PathBuf::from(target_dir).join(&temp_name);
            
            std::fs::create_dir_all(target_dir)?;
            let mut temp_file = std::fs::File::create(&temp_path)?;
            
            let mut sha256 = Sha256::new();
            
            // Constrain accumulation RAM buffer to 8MB max
            let mut ram_buffer = Vec::with_capacity(8 * 1024 * 1024);
            
            while let Some(chunk_res) = field.next().await {
                let chunk = chunk_res?;
                sha256.update(&chunk);
                
                if ram_buffer.len() + chunk.len() > 8 * 1024 * 1024 {
                    temp_file.write_all(&ram_buffer)?;
                    ram_buffer.clear();
                }
                ram_buffer.extend_from_slice(&chunk);
            }
            
            if !ram_buffer.is_empty() {
                temp_file.write_all(&ram_buffer)?;
            }
            
            temp_file.sync_all()?;
            drop(temp_file);
            
            let hash_str = format!("{:x}", sha256.finalize());
            let final_path = std::path::PathBuf::from(target_dir).join(&hash_str);
            std::fs::rename(&temp_path, &final_path)?;
            file_hash = hash_str;
        }
    }
    
    if file_hash.is_empty() {
        return Err(actix_web::error::ErrorBadRequest("No file parts detected in request"));
    }
    
    Ok(file_hash)
}

/// Runs non-blocking HLS transcoding with 4-second chunk segments using ffmpeg
pub async fn run_ffmpeg_transcode(input: &str, output: &str) -> Result<(), std::io::Error> {
    let status = tokio::process::Command::new("ffmpeg")
        .args(&[
            "-i", input,
            "-profile:v", "main",
            "-level", "3.0",
            "-start_number", "0",
            "-hls_time", "4",
            "-hls_list_size", "0",
            "-f", "hls",
            output
        ])
        .status()
        .await?;
        
    if !status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("FFmpeg failed with status code: {:?}", status.code())
        ));
    }
    Ok(())
}

/// Executes vector similarity search queries in SurrealDB using HNSW indexes
pub async fn search_transcripts(
    db: &surrealdb::Surreal<surrealdb::engine::any::Any>,
    query_embedding: &[f32; 1536],
    threshold: f32,
) -> Result<Vec<serde_json::Value>, surrealdb::Error> {
    let query = "SELECT *, vector::similarity::cosine(embedding, $query_vec) AS similarity \
                 FROM media_transcripts \
                 WHERE vector::similarity::cosine(embedding, $query_vec) >= $threshold \
                 ORDER BY similarity DESC;";
                 
    let vec_emb = query_embedding.to_vec();
    let mut response = db.query(query)
        .bind(("query_vec", vec_emb))
        .bind(("threshold", threshold))
        .await?;
        
    let results: Vec<serde_json::Value> = response.take(0)?;
    Ok(results)
}

/// Serves byte ranges supporting HTTP 206 Partial Content range requests
pub async fn serve_video_range(
    file_path: &std::path::Path,
    req: &actix_web::HttpRequest,
) -> Result<actix_web::HttpResponse, actix_web::Error> {
    if !file_path.exists() {
        return Ok(actix_web::HttpResponse::NotFound().body("Video not found"));
    }
    
    let file_len = match std::fs::metadata(file_path) {
        Ok(m) => m.len(),
        Err(e) => return Err(actix_web::error::ErrorInternalServerError(e)),
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
        return Ok(actix_web::HttpResponse::RangeNotSatisfiable()
            .insert_header(("Content-Range", format!("bytes */{}", file_len)))
            .finish());
    }
    
    use std::io::{Read, Seek, SeekFrom};
    let mut file = match std::fs::File::open(file_path) {
        Ok(f) => f,
        Err(e) => return Err(actix_web::error::ErrorInternalServerError(e)),
    };
    
    if let Err(e) = file.seek(SeekFrom::Start(start)) {
        return Err(actix_web::error::ErrorInternalServerError(e));
    }
    
    let chunk_len = end - start + 1;
    let mut buffer = vec![0; chunk_len as usize];
    if let Err(e) = file.read_exact(&mut buffer) {
        return Err(actix_web::error::ErrorInternalServerError(e));
    }
    
    if is_range {
        Ok(actix_web::HttpResponse::PartialContent()
            .insert_header(("Content-Range", format!("bytes {}-{}/{}", start, end, file_len)))
            .insert_header(("Content-Type", "video/mp4"))
            .body(buffer))
    } else {
        Ok(actix_web::HttpResponse::Ok()
            .insert_header(("Content-Length", file_len.to_string()))
            .insert_header(("Content-Type", "video/mp4"))
            .body(buffer))
    }
}

/// Generates signed URL using HMAC-SHA256
pub fn generate_signed_url(url: &str, secret: &[u8], expiry_secs: u64) -> String {
    let expires = chrono::Utc::now().timestamp() + expiry_secs as i64;
    let message = format!("{}:{}", url, expires);
    
    let mut mac = HmacSha256::new_from_slice(secret)
        .expect("HMAC keys can be of any size");
    mac.update(message.as_bytes());
    let signature = hex_encode(&mac.finalize().into_bytes());
    
    let separator = if url.contains('?') { "&" } else { "?" };
    format!("{}{}_token={}&_expires={}", url, separator, signature, expires)
}

/// Verifies signed URL HMAC signature
pub fn verify_signed_url(url: &str, secret: &[u8]) -> Result<(), &'static str> {
    let mut token = "";
    let mut expires_str = "";
    
    let pos = url.find('?').unwrap_or(url.len());
    let base = &url[..pos];
    let query = if pos < url.len() { &url[pos + 1..] } else { "" };
    
    let mut other_params = Vec::new();
    
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
            if k == "_token" {
                token = v;
            } else if k == "_expires" {
                expires_str = v;
            } else {
                other_params.push(pair);
            }
        }
    }
    
    if token.is_empty() || expires_str.is_empty() {
        return Err("Missing signature parameters");
    }
    
    let expires = expires_str.parse::<i64>().map_err(|_| "Invalid expiration format")?;
    let now = chrono::Utc::now().timestamp();
    if now > expires {
        return Err("Signature has expired");
    }
    
    let reconstructed_url = if other_params.is_empty() {
        base.to_string()
    } else {
        format!("{}?{}", base, other_params.join("&"))
    };
    
    let message = format!("{}:{}", reconstructed_url, expires);
    
    let mut mac = HmacSha256::new_from_slice(secret)
        .map_err(|_| "HMAC initialization failed")?;
    mac.update(message.as_bytes());
    
    let token_bytes = hex_decode(token).map_err(|_| "Invalid hex token format")?;
    if mac.verify_slice(&token_bytes).is_err() {
        return Err("Invalid signature");
    }
    
    Ok(())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn hex_decode(hex_str: &str) -> Result<Vec<u8>, &'static str> {
    if hex_str.len() % 2 != 0 {
        return Err("Hex string must have even length");
    }
    let mut bytes = Vec::with_capacity(hex_str.len() / 2);
    for i in (0..hex_str.len()).step_by(2) {
        let chunk = &hex_str[i..i+2];
        let byte = u8::from_str_radix(chunk, 16).map_err(|_| "Invalid hex byte")?;
        bytes.push(byte);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_youtube_parsing() {
        let url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
        let info = parse_video_url(url).unwrap();
        assert_eq!(info.platform, VideoPlatform::YouTube);
        assert_eq!(info.video_id, "dQw4w9WgXcQ");

        let short_url = "https://youtu.be/dQw4w9WgXcQ";
        let info_short = parse_video_url(short_url).unwrap();
        assert_eq!(info_short.video_id, "dQw4w9WgXcQ");
    }

    #[test]
    fn test_vimeo_parsing() {
        let url = "https://vimeo.com/838150493";
        let info = parse_video_url(url).unwrap();
        assert_eq!(info.platform, VideoPlatform::Vimeo);
        assert_eq!(info.video_id, "838150493");
    }

    #[test]
    fn test_html5_parsing() {
        let url = "https://example.com/assets/intro.mp4";
        let info = parse_video_url(url).unwrap();
        assert_eq!(info.platform, VideoPlatform::Html5);
        assert_eq!(info.video_id, url);
    }

    #[test]
    fn test_custom_signed_url() {
        let url = "http://localhost/video/sample.mp4";
        let secret = b"supersecret";
        let signed = generate_signed_url(url, secret, 3600);
        assert!(verify_signed_url(&signed, secret).is_ok());
        
        let invalid_secret = b"wrongsecret";
        assert!(verify_signed_url(&signed, invalid_secret).is_err());
    }
}
