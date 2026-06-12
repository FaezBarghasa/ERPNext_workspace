use hmac::{Hmac, Mac};
use sha2::Sha256;
use chrono::Utc;
use std::collections::HashMap;

type HmacSha256 = Hmac<Sha256>;

#[derive(thiserror::Error, Debug)]
pub enum SecurityError {
    #[error("Missing query parameter: {0}")]
    MissingParameter(String),
    #[error("Invalid signature format: {0}")]
    InvalidSignatureFormat(String),
    #[error("Invalid expires timestamp format: {0}")]
    InvalidExpiresFormat(String),
    #[error("Link signature has expired")]
    Expired,
    #[error("Invalid signature verification failed")]
    InvalidSignature,
    #[error("Failed to parse URL: {0}")]
    UrlParseError(String),
}

/// Helper to parse query parameters from a URL string manually without heavy url crate dependencies
fn parse_query_params(url: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();
    if let Some(pos) = url.find('?') {
        let query = &url[pos + 1..];
        for pair in query.split('&') {
            let mut parts = pair.splitn(2, '=');
            if let (Some(key), Some(val)) = (parts.next(), parts.next()) {
                params.insert(key.to_string(), val.to_string());
            }
        }
    }
    params
}

/// Generates a signed URL for a specific file hash and expiration duration.
pub fn generate_signed_url(
    base_url: &str,
    file_hash: &str,
    secret: &[u8],
    expires_in_secs: u64,
) -> String {
    let expires = Utc::now().timestamp() + expires_in_secs as i64;
    let message = format!("{}:{}", file_hash, expires);

    // Compute HMAC-SHA256 signature
    let mut mac = HmacSha256::new_from_slice(secret)
        .expect("HMAC keys can be of any size");
    mac.update(message.as_bytes());
    let signature = hex::encode(&mac.finalize().into_bytes());

    let separator = if base_url.contains('?') { "&" } else { "?" };
    format!(
        "{}{}_token={}&_expires={}&_hash={}",
        base_url, separator, signature, expires, file_hash
    )
}

/// Validates a signed URL's HMAC signature and checks if it has expired.
/// On success, returns the validated file hash.
pub fn validate_signed_url(signed_url: &str, secret: &[u8]) -> Result<String, SecurityError> {
    let params = parse_query_params(signed_url);

    let token = params
        .get("_token")
        .ok_or_else(|| SecurityError::MissingParameter("_token".to_string()))?;
    let expires_str = params
        .get("_expires")
        .ok_or_else(|| SecurityError::MissingParameter("_expires".to_string()))?;
    let file_hash = params
        .get("_hash")
        .ok_or_else(|| SecurityError::MissingParameter("_hash".to_string()))?;

    // Parse expiration timestamp
    let expires = expires_str
        .parse::<i64>()
        .map_err(|_| SecurityError::InvalidExpiresFormat(expires_str.clone()))?;

    // Check expiration
    let now = Utc::now().timestamp();
    if now > expires {
        return Err(SecurityError::Expired);
    }

    // Decode token
    let token_bytes = hex::decode(token)
        .map_err(|_| SecurityError::InvalidSignatureFormat(token.clone()))?;

    // Recompute signature
    let message = format!("{}:{}", file_hash, expires);
    let mut mac = HmacSha256::new_from_slice(secret)
        .map_err(|_| SecurityError::InvalidSignature)?;
    mac.update(message.as_bytes());

    if mac.verify_slice(&token_bytes).is_err() {
        return Err(SecurityError::InvalidSignature);
    }

    Ok(file_hash.clone())
}

/// Generates an HTML embed tag for private HTML5 videos utilizing secure HMAC URL signatures.
pub fn generate_private_embed_html(
    base_url: &str,
    file_hash: &str,
    secret: &[u8],
    expires_in_secs: u64,
    options: &crate::video_player::EmbedOptions,
) -> String {
    let signed_url = generate_signed_url(base_url, file_hash, secret, expires_in_secs);
    let info = crate::video_player::VideoInfo {
        platform: crate::video_player::VideoPlatform::Html5,
        video_id: signed_url,
        original_url: base_url.to_string(),
    };
    crate::video_player::generate_embed_html(&info, options)
}


mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }

    pub fn decode(hex_str: &str) -> Result<Vec<u8>, String> {
        if hex_str.len() % 2 != 0 {
            return Err("Hex string must have even length".to_string());
        }
        let mut bytes = Vec::with_capacity(hex_str.len() / 2);
        for i in (0..hex_str.len()).step_by(2) {
            let chunk = &hex_str[i..i + 2];
            let byte = u8::from_str_radix(chunk, 16)
                .map_err(|e| e.to_string())?;
            bytes.push(byte);
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_signed_url_success() {
        let base_url = "http://localhost/video";
        let file_hash = "abc123hashxyz";
        let secret = b"my_secret_key";
        
        let signed = generate_signed_url(base_url, file_hash, secret, 3600);
        let validated = validate_signed_url(&signed, secret).unwrap();
        assert_eq!(validated, file_hash);
    }

    #[test]
    fn test_signed_url_expired() {
        let base_url = "http://localhost/video";
        let file_hash = "abc123hashxyz";
        let secret = b"my_secret_key";

        // URL expires in 0 seconds (instantly)
        let signed = generate_signed_url(base_url, file_hash, secret, 0);
        // Sleep 1 second to ensure expiration triggers
        thread::sleep(Duration::from_secs(1));
        let res = validate_signed_url(&signed, secret);
        assert!(matches!(res, Err(SecurityError::Expired)));
    }

    #[test]
    fn test_signed_url_invalid_signature() {
        let base_url = "http://localhost/video";
        let file_hash = "abc123hashxyz";
        let secret = b"my_secret_key";

        let signed = generate_signed_url(base_url, file_hash, secret, 3600);
        
        // Mutate signature slightly
        let mutated = signed.replace("_token=", "_token=ff");
        let res = validate_signed_url(&mutated, secret);
        assert!(matches!(res, Err(SecurityError::InvalidSignature) | Err(SecurityError::InvalidSignatureFormat(_))));
    }
}
