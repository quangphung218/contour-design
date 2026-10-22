//! MP4 encoding of captured frames with the ffmpeg already installed
//! (spec 4.2, MP4 pipeline step 5): libx264, `yuv420p`, `+faststart`, size
//! padded to even numbers, 5 minute timeout.

use od_export::renderer::FRAME_FILE_PATTERN;
use od_export::{RenderError, RenderResult};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;

/// Environment variable naming the ffmpeg executable.
pub const FFMPEG_PATH_ENV: &str = "OD_FFMPEG_PATH";
/// Encoding must finish within 5 minutes.
pub const ENCODE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// Last bytes of ffmpeg's stderr kept for the error message.
const STDERR_TAIL: usize = 8_000;

/// Finds ffmpeg: `OD_FFMPEG_PATH`, the Homebrew and system locations, then
/// `PATH`.
///
/// # Errors
/// [`RenderError::Unavailable`] naming what to install or set.
pub fn find_ffmpeg() -> RenderResult<PathBuf> {
    if let Some(path) = std::env::var_os(FFMPEG_PATH_ENV).filter(|v| !v.is_empty()) {
        let path = PathBuf::from(path);
        return if path.is_file() {
            Ok(path)
        } else {
            Err(RenderError::Unavailable(format!(
                "{FFMPEG_PATH_ENV} points to {}, which does not exist",
                path.display()
            )))
        };
    }
    let name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"]
        .iter()
        .map(|d| Path::new(d).join(name))
        .find(|p| p.is_file())
        .or_else(|| {
            let path = std::env::var_os("PATH")?;
            std::env::split_paths(&path)
                .map(|d| d.join(name))
                .find(|p| p.is_file())
        })
        .ok_or_else(|| {
            RenderError::Unavailable(format!(
                "MP4 export needs ffmpeg; install it or set {FFMPEG_PATH_ENV}"
            ))
        })
}

/// Arguments that turn `frames_dir/frame-%08d.png` at `fps` into `output`.
#[must_use]
pub fn encode_args(frames_dir: &Path, fps: u32, output: &Path) -> Vec<String> {
    let input = frames_dir.join(FRAME_FILE_PATTERN);
    [
        "-y",
        "-hide_banner",
        "-loglevel",
        "error",
        "-framerate",
        &fps.to_string(),
        "-start_number",
        "1",
        "-i",
        &input.to_string_lossy(),
        "-vf",
        "pad=ceil(iw/2)*2:ceil(ih/2)*2",
        "-c:v",
        "libx264",
        "-pix_fmt",
        "yuv420p",
        "-movflags",
        "+faststart",
        &output.to_string_lossy(),
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect()
}

/// Encodes the frames in `frames_dir` into `output`.
///
/// # Errors
/// [`RenderError::Unavailable`] without ffmpeg; [`RenderError::Failed`] on
/// a non-zero exit (with the stderr tail) or after [`ENCODE_TIMEOUT`].
pub async fn encode_mp4(
    ffmpeg: &Path,
    frames_dir: &Path,
    fps: u32,
    output: &Path,
) -> RenderResult<()> {
    let mut child = tokio::process::Command::new(ffmpeg)
        .args(encode_args(frames_dir, fps, output))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| RenderError::Unavailable(format!("cannot start {}: {e}", ffmpeg.display())))?;
    let mut stderr = child.stderr.take();
    let read_stderr = async {
        let mut buf = Vec::new();
        if let Some(err) = stderr.as_mut() {
            let _ = err.read_to_end(&mut buf).await;
        }
        buf
    };
    let run = async { tokio::join!(child.wait(), read_stderr) };
    // On timeout the child is dropped, which kills it (`kill_on_drop`).
    let Ok((status, err)) = tokio::time::timeout(ENCODE_TIMEOUT, run).await else {
        return Err(RenderError::Failed(format!(
            "ffmpeg encoding timed out after {} minutes",
            ENCODE_TIMEOUT.as_secs() / 60
        )));
    };
    let status = status.map_err(|e| RenderError::Failed(format!("waiting for ffmpeg: {e}")))?;
    if status.success() {
        return Ok(());
    }
    let tail = &err[err.len().saturating_sub(STDERR_TAIL)..];
    Err(RenderError::Failed(format!(
        "ffmpeg encoding failed ({status}): {}",
        String::from_utf8_lossy(tail).trim()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_follow_the_pipeline_contract() {
        let args = encode_args(Path::new("/f"), 30, Path::new("/o.mp4"));
        let joined = args.join(" ");
        for part in [
            "-framerate 30",
            "-c:v libx264",
            "-pix_fmt yuv420p",
            "-movflags +faststart",
            "pad=ceil(iw/2)*2:ceil(ih/2)*2",
            "/f/frame-%08d.png",
        ] {
            assert!(joined.contains(part), "{joined}");
        }
        assert_eq!(args.last().map(String::as_str), Some("/o.mp4"));
    }
}
