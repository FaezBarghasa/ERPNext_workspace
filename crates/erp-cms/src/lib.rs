pub mod video_player;

pub use video_player::{
    parse_video_url, generate_embed_html, VideoPlatform, VideoInfo, EmbedOptions, VideoError
};
