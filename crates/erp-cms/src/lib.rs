pub mod video_player;
pub mod security;
pub mod transcoder;

pub use video_player::{
    parse_video_url, generate_embed_html, VideoPlatform, VideoInfo, EmbedOptions, VideoError,
    handle_multipart_upload, run_ffmpeg_transcode, search_transcripts, serve_video_range,
    verify_signed_url,
    generate_signed_url as generate_signed_url_svod,
};
pub use security::{generate_signed_url, validate_signed_url, generate_private_embed_html, SecurityError};
pub use transcoder::{VideoTranscoder, VideoFormat, TranscoderError};
