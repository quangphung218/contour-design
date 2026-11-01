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
    find_ffmpeg_with(std::env::var_os(FFMPEG_PATH_ENV))
}

/// [`find_ffmpeg`] with `configured` in place of `OD_FFMPEG_PATH`.
///
/// # Errors
/// [`RenderError::Unavailable`] naming what to install or set.
pub fn find_ffmpeg_with(configured: Option<std::ffi::OsString>) -> RenderResult<PathBuf> {
    if let Some(path) = configured.filter(|v| !v.is_empty()) {
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

/// One local audio file to mix into the MP4, times in film seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioTrack {
    pub path: PathBuf,
    /// ffmpeg demuxer for the file ([`audio_format`]): set explicitly so
    /// ffmpeg never probes a playlist that could reach the network.
    pub format: &'static str,
    /// When the track starts in the film.
    pub start: f64,
    /// How much of the file plays; `None` plays it to its end.
    pub duration: Option<f64>,
    /// Gain, 0–1.
    pub volume: f64,
}

/// The ffmpeg demuxer for an audio file, by extension; `None` for
/// anything that is not a plain media file (playlists included).
#[must_use]
pub fn audio_format(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "wav" => "wav",
        "mp3" => "mp3",
        "ogg" | "oga" | "opus" => "ogg",
        "flac" => "flac",
        "aac" => "aac",
        "m4a" | "mp4" | "mov" => "mov",
        "webm" => "matroska",
        _ => return None,
    })
}

/// Seconds as ffmpeg reads them: millisecond precision, no exponent.
fn secs(v: f64) -> String {
    format!("{:.3}", v.max(0.0))
}

/// The `-filter_complex` graph that delays, trims and scales each track,
/// mixes them without averaging (`amix normalize=0`) and pads or cuts the
/// result to `film_seconds`. Input 0 is the video.
#[must_use]
pub fn audio_filter(tracks: &[AudioTrack], film_seconds: f64) -> String {
    let mut parts: Vec<String> = tracks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let trim = t
                .duration
                .map(|d| format!("atrim=duration={},", secs(d)))
                .unwrap_or_default();
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped ≥ 0
            let delay = (t.start.max(0.0) * 1000.0).round() as u64;
            format!(
                "[{n}:a]aformat=channel_layouts=stereo,{trim}asetpts=PTS-STARTPTS,volume={vol},adelay={delay}:all=1[a{i}]",
                n = i + 1,
                vol = secs(t.volume),
            )
        })
        .collect();
    let inputs: String = (0..tracks.len()).map(|i| format!("[a{i}]")).collect();
    let mixed = if tracks.len() == 1 {
        inputs
    } else {
        format!(
            "{inputs}amix=inputs={}:duration=longest:dropout_transition=0:normalize=0,",
            tracks.len()
        )
    };
    parts.push(format!(
        "{mixed}apad,atrim=duration={}[aout]",
        secs(film_seconds)
    ));
    parts.join(";")
}

/// Arguments that turn `frames_dir/frame-%08d.png` at `fps` into `output`.
#[must_use]
pub fn encode_args(frames_dir: &Path, fps: u32, output: &Path) -> Vec<String> {
    encode_args_with_audio(frames_dir, fps, output, &[], 0.0)
}

/// [`encode_args`] plus `tracks` mixed into one AAC stream as long as the
/// film (`film_seconds`). No tracks: exactly the picture-only arguments.
#[must_use]
pub fn encode_args_with_audio(
    frames_dir: &Path,
    fps: u32,
    output: &Path,
    tracks: &[AudioTrack],
    film_seconds: f64,
) -> Vec<String> {
    let input = frames_dir.join(FRAME_FILE_PATTERN);
    let mut args: Vec<String> = [
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
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    for t in tracks {
        args.extend([
            "-f".to_owned(),
            t.format.to_owned(),
            "-i".to_owned(),
            t.path.to_string_lossy().into_owned(),
        ]);
    }
    args.extend(
        [
            "-vf",
            "pad=ceil(iw/2)*2:ceil(ih/2)*2",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
        ]
        .map(str::to_owned),
    );
    if !tracks.is_empty() {
        args.extend([
            "-filter_complex".to_owned(),
            audio_filter(tracks, film_seconds),
            "-map".to_owned(),
            "0:v".to_owned(),
            "-map".to_owned(),
            "[aout]".to_owned(),
            "-c:a".to_owned(),
            "aac".to_owned(),
            "-b:a".to_owned(),
            "192k".to_owned(),
        ]);
    }
    args.extend(["-movflags", "+faststart"].map(str::to_owned));
    args.push(output.to_string_lossy().into_owned());
    args
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
    encode_mp4_with_audio(ffmpeg, frames_dir, fps, output, &[], 0.0).await
}

/// [`encode_mp4`] with `tracks` mixed in as one AAC stream cut to
/// `film_seconds` ([`encode_args_with_audio`]).
///
/// # Errors
/// As [`encode_mp4`]; a track ffmpeg cannot decode fails the encode.
pub async fn encode_mp4_with_audio(
    ffmpeg: &Path,
    frames_dir: &Path,
    fps: u32,
    output: &Path,
    tracks: &[AudioTrack],
    film_seconds: f64,
) -> RenderResult<()> {
    let mut child = tokio::process::Command::new(ffmpeg)
        .args(encode_args_with_audio(
            frames_dir,
            fps,
            output,
            tracks,
            film_seconds,
        ))
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
        ] {
            assert!(joined.contains(part), "{joined}");
        }
        let frames = Path::new("/f").join("frame-%08d.png");
        assert!(joined.contains(&*frames.to_string_lossy()), "{joined}");
        assert_eq!(args.last().map(String::as_str), Some("/o.mp4"));
    }

    #[test]
    fn audio_args_mix_every_track_into_one_aac_stream() {
        let tracks = [
            AudioTrack {
                path: PathBuf::from("/c/music.wav"),
                format: "wav",
                start: 0.0,
                duration: Some(10.0),
                volume: 0.3,
            },
            AudioTrack {
                path: PathBuf::from("/c/voice.mp3"),
                format: "mp3",
                start: 2.0,
                duration: None,
                volume: 1.0,
            },
        ];
        let args = encode_args_with_audio(Path::new("/f"), 30, Path::new("/o.mp4"), &tracks, 10.0);
        let joined = args.join(" ");
        for part in [
            "-f wav -i /c/music.wav",
            "-f mp3 -i /c/voice.mp3",
            "-map 0:v -map [aout]",
            "-c:a aac",
            "-c:v libx264",
        ] {
            assert!(joined.contains(part), "{joined}");
        }
        let graph = audio_filter(&tracks, 10.0);
        assert_eq!(
            graph,
            "[1:a]aformat=channel_layouts=stereo,atrim=duration=10.000,asetpts=PTS-STARTPTS,volume=0.300,adelay=0:all=1[a0];\
             [2:a]aformat=channel_layouts=stereo,asetpts=PTS-STARTPTS,volume=1.000,adelay=2000:all=1[a1];\
             [a0][a1]amix=inputs=2:duration=longest:dropout_transition=0:normalize=0,apad,atrim=duration=10.000[aout]"
        );
        assert_eq!(
            audio_filter(&tracks[..1], 4.5).rsplit(';').next(),
            Some("[a0]apad,atrim=duration=4.500[aout]")
        );
        assert_eq!(args.last().map(String::as_str), Some("/o.mp4"));
    }

    #[test]
    fn no_tracks_keeps_the_picture_only_arguments() {
        let (f, o) = (Path::new("/f"), Path::new("/o.mp4"));
        assert_eq!(
            encode_args_with_audio(f, 24, o, &[], 3.0),
            encode_args(f, 24, o)
        );
        let joined = encode_args(f, 24, o).join(" ");
        assert!(
            !joined.contains("-c:a") && !joined.contains("-map"),
            "{joined}"
        );
    }

    #[test]
    fn audio_formats_are_plain_media_files() {
        assert_eq!(audio_format(Path::new("a/B.WAV")), Some("wav"));
        assert_eq!(audio_format(Path::new("a.m4a")), Some("mov"));
        assert_eq!(audio_format(Path::new("a.m3u8")), None);
        assert_eq!(audio_format(Path::new("a")), None);
    }
}
