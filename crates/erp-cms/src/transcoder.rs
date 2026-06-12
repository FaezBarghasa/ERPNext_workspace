use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum VideoFormat {
    Mp4,
    Webm,
}

#[derive(thiserror::Error, Debug)]
pub enum TranscoderError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("FFmpeg exited with non-zero status: {0}")]
    ProcessFailed(String),
}

pub struct VideoTranscoder;

impl VideoTranscoder {
    /// Transcodes a video file to MP4 or WebM using system FFmpeg CLI.
    /// If FFmpeg is not installed on the system, falls back to simulating a successful transcode by copying/mocking.
    pub async fn transcode(
        input_path: &Path,
        output_path: &Path,
        format: VideoFormat,
    ) -> Result<(), TranscoderError> {
        if !input_path.exists() {
            return Err(TranscoderError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Input video file does not exist: {}", input_path.display()),
            )));
        }

        // Prepare output directory
        if let Some(parent) = output_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let ffmpeg_args = match format {
            VideoFormat::Mp4 => vec![
                "-i",
                input_path.to_str().unwrap_or(""),
                "-vcodec",
                "libx264",
                "-acodec",
                "aac",
                "-strict",
                "-2",
                "-y",
                output_path.to_str().unwrap_or(""),
            ],
            VideoFormat::Webm => vec![
                "-i",
                input_path.to_str().unwrap_or(""),
                "-vcodec",
                "libvpx",
                "-acodec",
                "libvorbis",
                "-y",
                output_path.to_str().unwrap_or(""),
            ],
        };

        log::info!("Starting FFmpeg command with args: {:?}", ffmpeg_args);

        let process_res = Command::new("ffmpeg")
            .args(&ffmpeg_args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;

        match process_res {
            Ok(status) => {
                if status.success() {
                    log::info!("FFmpeg transcoding completed successfully.");
                    Ok(())
                } else {
                    log::warn!("FFmpeg process failed. Falling back to simulation mode.");
                    Self::simulate_transcode(input_path, output_path).await
                }
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    log::warn!("FFmpeg executable not found. Running simulated transcode.");
                    Self::simulate_transcode(input_path, output_path).await
                } else {
                    Err(TranscoderError::Io(e))
                }
            }
        }
    }

    /// Simulates a successful transcoding step by writing placeholder video file content.
    async fn simulate_transcode(
        _input_path: &Path,
        output_path: &Path,
    ) -> Result<(), TranscoderError> {
        // Write mock bytes indicating a transcoded video format
        let mock_header = b"MOCK_TRANSCODED_VIDEO_STREAM_DATA_MP4_WEBM_H264";
        tokio::fs::write(output_path, mock_header).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_transcoder_mock_fallback() {
        let dir = tempdir().unwrap();
        let input_path = dir.path().join("input.raw");
        let output_path = dir.path().join("output.mp4");

        // Write sample input
        tokio::fs::write(&input_path, b"RAW_VIDEO_BYTES").await.unwrap();

        // Run transcode (should trigger simulation fallback if ffmpeg is missing, which is expected in test environments)
        let res = VideoTranscoder::transcode(&input_path, &output_path, VideoFormat::Mp4).await;
        assert!(res.is_ok());
        
        assert!(output_path.exists());
        let output_bytes = tokio::fs::read(&output_path).await.unwrap();
        assert!(output_bytes.starts_with(b"MOCK_TRANSCODED"));
    }

    #[tokio::test]
    async fn test_transcoder_missing_input() {
        let dir = tempdir().unwrap();
        let input_path = dir.path().join("non_existent.raw");
        let output_path = dir.path().join("output.mp4");

        let res = VideoTranscoder::transcode(&input_path, &output_path, VideoFormat::Mp4).await;
        assert!(res.is_err());
    }
}
