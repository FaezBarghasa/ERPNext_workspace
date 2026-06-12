use serde::{Deserialize, Serialize};
use regex::Regex;

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
    // 1. youtube.com/watch?v=ID
    // 2. youtu.be/ID
    // 3. youtube.com/embed/ID
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
    // 1. vimeo.com/ID
    // 2. player.vimeo.com/video/ID
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
                // YouTube requires muting to autoplay
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
}
