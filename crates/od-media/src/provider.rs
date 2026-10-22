//! Provider adapters (spec 1.5 media gen, 4.2: plain HTTP, no browser):
//! OpenAI Images, fal.ai's queue API (image and video), ElevenLabs
//! text-to-speech and sound effects. Request and response shapes follow
//! the original `media/index.ts`. Every call uses the user's own key and
//! the base URL they configured (tests point it at a loopback fake).

use crate::config::Credentials;
use base64::Engine;
use serde::Serialize;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    #[serde(rename = "openai")]
    OpenAi,
    Fal,
    #[serde(rename = "elevenlabs")]
    ElevenLabs,
}

impl Provider {
    pub const ALL: [Self; 3] = [Self::OpenAi, Self::Fal, Self::ElevenLabs];

    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Fal => "fal",
            Self::ElevenLabs => "elevenlabs",
        }
    }

    #[must_use]
    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.id() == id)
    }

    /// Environment variables holding a key, highest precedence first.
    #[must_use]
    pub fn env_keys(self) -> &'static [&'static str] {
        match self {
            Self::OpenAi => &["OD_OPENAI_API_KEY", "OPENAI_API_KEY"],
            Self::Fal => &["OD_FAL_KEY", "FAL_KEY"],
            Self::ElevenLabs => &["OD_ELEVENLABS_API_KEY", "ELEVENLABS_API_KEY"],
        }
    }

    #[must_use]
    pub fn default_base_url(self) -> &'static str {
        match self {
            Self::OpenAi => "https://api.openai.com/v1",
            Self::Fal => "https://queue.fal.run",
            Self::ElevenLabs => "https://api.elevenlabs.io",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Surface {
    Image,
    Video,
    Audio,
}

/// One catalogue model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ModelDef {
    pub id: &'static str,
    pub label: &'static str,
    pub provider: Provider,
    pub surface: Surface,
    /// Wire name or fal endpoint.
    #[serde(skip)]
    pub wire: &'static str,
}

const fn m(
    id: &'static str,
    label: &'static str,
    provider: Provider,
    surface: Surface,
    wire: &'static str,
) -> ModelDef {
    ModelDef {
        id,
        label,
        provider,
        surface,
        wire,
    }
}

/// The models this build can call (the original's OpenAI, fal and
/// ElevenLabs entries).
pub const MODELS: &[ModelDef] = &[
    m(
        "gpt-image-2",
        "gpt-image-2",
        Provider::OpenAi,
        Surface::Image,
        "gpt-image-2",
    ),
    m(
        "gpt-image-1.5",
        "gpt-image-1.5",
        Provider::OpenAi,
        Surface::Image,
        "gpt-image-1.5",
    ),
    m(
        "gpt-image-1",
        "gpt-image-1",
        Provider::OpenAi,
        Surface::Image,
        "gpt-image-1",
    ),
    m(
        "gpt-image-1-mini",
        "gpt-image-1-mini",
        Provider::OpenAi,
        Surface::Image,
        "gpt-image-1-mini",
    ),
    m(
        "dall-e-3",
        "dall-e-3",
        Provider::OpenAi,
        Surface::Image,
        "dall-e-3",
    ),
    m(
        "dall-e-2",
        "dall-e-2",
        Provider::OpenAi,
        Surface::Image,
        "dall-e-2",
    ),
    m(
        "flux-pro-ultra",
        "flux-pro-ultra",
        Provider::Fal,
        Surface::Image,
        "fal-ai/flux-pro/v1.1-ultra",
    ),
    m(
        "flux-dev-fal",
        "flux-dev (fal)",
        Provider::Fal,
        Surface::Image,
        "fal-ai/flux/dev",
    ),
    m(
        "flux-schnell-fal",
        "flux-schnell (fal)",
        Provider::Fal,
        Surface::Image,
        "fal-ai/flux/schnell",
    ),
    m(
        "sd-3.5",
        "Stable Diffusion 3.5",
        Provider::Fal,
        Surface::Image,
        "fal-ai/stable-diffusion-v35-large",
    ),
    m(
        "ideogram-v3-fal",
        "ideogram-v3 (fal)",
        Provider::Fal,
        Surface::Image,
        "fal-ai/ideogram/v3",
    ),
    m(
        "recraft-v3-fal",
        "recraft-v3 (fal)",
        Provider::Fal,
        Surface::Image,
        "fal-ai/recraft-v3",
    ),
    m(
        "sora-2",
        "sora-2 (fal)",
        Provider::Fal,
        Surface::Video,
        "fal-ai/sora",
    ),
    m(
        "veo-3-fal",
        "veo-3 (fal)",
        Provider::Fal,
        Surface::Video,
        "fal-ai/veo3",
    ),
    m(
        "veo-2-fal",
        "veo-2 (fal)",
        Provider::Fal,
        Surface::Video,
        "fal-ai/veo2",
    ),
    m(
        "wan-2.1-t2v",
        "wan-2.1 t2v (fal)",
        Provider::Fal,
        Surface::Video,
        "fal-ai/wan-t2v",
    ),
    m(
        "seedance-1-pro-fal",
        "seedance-1-pro (fal)",
        Provider::Fal,
        Surface::Video,
        "fal-ai/bytedance/seedance-1-pro",
    ),
    m(
        "kling-2.1-t2v-fal",
        "kling-2.1 t2v (fal)",
        Provider::Fal,
        Surface::Video,
        "fal-ai/kling-video/v2.1/master/text-to-video",
    ),
    m(
        "elevenlabs-v3",
        "ElevenLabs v3 (speech)",
        Provider::ElevenLabs,
        Surface::Audio,
        "eleven_v3",
    ),
    m(
        "elevenlabs-sfx",
        "ElevenLabs sound effects",
        Provider::ElevenLabs,
        Surface::Audio,
        "eleven_text_to_sound_v2",
    ),
];

/// Resolves a model id; any `fal-ai/…` path goes to fal verbatim.
#[must_use]
pub fn find_model(id: &str, surface: Surface) -> Option<ModelDef> {
    if let Some(def) = MODELS.iter().find(|d| d.id == id) {
        return Some(*def);
    }
    id.starts_with("fal-ai/").then_some(ModelDef {
        id: "fal-ai/custom",
        label: "fal custom",
        provider: Provider::Fal,
        surface,
        wire: "",
    })
}

/// What a generation asks for.
#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub surface: Surface,
    pub model: String,
    pub prompt: String,
    pub aspect: Option<String>,
    /// Video length in seconds.
    pub length: Option<f64>,
    /// Sound-effect duration in seconds.
    pub duration: Option<f64>,
    pub voice: Option<String>,
}

/// Provider bytes plus what to call the file.
#[derive(Debug, Clone, PartialEq)]
pub struct Rendered {
    pub bytes: Vec<u8>,
    /// With the dot: `.png`, `.mp4`, `.mp3`.
    pub ext: &'static str,
    pub note: String,
}

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("{0}")]
    BadRequest(String),
    #[error("no {provider} API key: set it in Settings → Media or the {env} environment variable")]
    NoCredential {
        provider: &'static str,
        env: &'static str,
    },
    #[error("{0}")]
    Upstream(String),
    #[error("{0}")]
    Timeout(String),
}

/// Upstream bodies are cut in messages.
fn truncate(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    if t.len() < s.len() {
        format!("{t}…")
    } else {
        t
    }
}

fn upstream(tag: &str, e: impl std::fmt::Display) -> MediaError {
    MediaError::Upstream(format!("{tag}: {e}"))
}

/// Shared client: generous timeout (image models take minutes).
///
/// # Errors
/// TLS backend initialisation failure.
pub fn http_client() -> Result<reqwest::Client, MediaError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10 * 60))
        .connect_timeout(Duration::from_secs(30))
        .user_agent(concat!("open-design-rs/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| upstream("http client", e))
}

/// Tunables (tests shorten the fal poll interval).
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub fal_poll_interval: Duration,
    pub fal_max_poll: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            fal_poll_interval: Duration::from_secs(3),
            fal_max_poll: Duration::from_secs(10 * 60),
        }
    }
}

fn key(creds: &Credentials, provider: Provider) -> Result<&str, MediaError> {
    creds.api_key.as_deref().ok_or(MediaError::NoCredential {
        provider: provider.id(),
        env: provider.env_keys()[1],
    })
}

fn base(creds: &Credentials, provider: Provider) -> String {
    creds
        .base_url
        .as_deref()
        .unwrap_or(provider.default_base_url())
        .trim_end_matches('/')
        .to_owned()
}

async fn body_text(res: reqwest::Response, tag: &str) -> Result<String, MediaError> {
    let status = res.status();
    let text = res.text().await.map_err(|e| upstream(tag, e))?;
    if status.is_success() {
        Ok(text)
    } else {
        Err(MediaError::Upstream(format!(
            "{tag} {}: {}",
            status.as_u16(),
            truncate(&text, 240)
        )))
    }
}

async fn json_of(res: reqwest::Response, tag: &str) -> Result<Value, MediaError> {
    let text = body_text(res, tag).await?;
    serde_json::from_str(&text).map_err(|_| {
        MediaError::Upstream(format!("{tag} non-JSON response: {}", truncate(&text, 200)))
    })
}

async fn download(http: &reqwest::Client, url: &str, tag: &str) -> Result<Vec<u8>, MediaError> {
    let res = http.get(url).send().await.map_err(|e| upstream(tag, e))?;
    if !res.status().is_success() {
        return Err(MediaError::Upstream(format!(
            "{tag} {}",
            res.status().as_u16()
        )));
    }
    Ok(res.bytes().await.map_err(|e| upstream(tag, e))?.to_vec())
}

/// Image extension from magic bytes.
#[must_use]
pub fn sniff_image_ext(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"\x89PNG") {
        ".png"
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        ".jpg"
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        ".webp"
    } else if bytes.starts_with(b"GIF8") {
        ".gif"
    } else {
        ".png"
    }
}

fn openai_size(model: &str, aspect: Option<&str>) -> &'static str {
    if model.starts_with("gpt-image-") {
        return match aspect {
            Some("16:9") => "1792x1024",
            Some("9:16") => "1024x1792",
            Some("4:3") => "1408x1056",
            Some("3:4") => "1056x1408",
            _ => "1024x1024",
        };
    }
    match (model, aspect) {
        ("dall-e-3", Some("16:9")) => "1792x1024",
        ("dall-e-3", Some("9:16")) => "1024x1792",
        _ => "1024x1024",
    }
}

async fn openai_image(
    http: &reqwest::Client,
    def: ModelDef,
    job: &Job,
    creds: &Credentials,
) -> Result<Rendered, MediaError> {
    let key = key(creds, Provider::OpenAi)?;
    let wire = creds.model.as_deref().unwrap_or(def.wire);
    let mut body = json!({
        "model": wire,
        "prompt": if job.prompt.is_empty() { "A high-quality reference image." } else { &job.prompt },
        "n": 1,
        "size": openai_size(def.id, job.aspect.as_deref()),
    });
    if def.id.starts_with("dall-e-") {
        body["response_format"] = json!("b64_json");
        body["quality"] = json!(if def.id == "dall-e-3" {
            "hd"
        } else {
            "standard"
        });
    } else {
        body["quality"] = json!("high");
    }
    let url = format!("{}/images/generations", base(creds, Provider::OpenAi));
    let res = http
        .post(&url)
        .bearer_auth(key)
        .json(&body)
        .send()
        .await
        .map_err(|e| upstream("openai", e))?;
    let data = json_of(res, "openai").await?;
    let entry = data
        .get("data")
        .and_then(|d| d.get(0))
        .ok_or_else(|| MediaError::Upstream("openai response had no data[0]".to_owned()))?;
    let bytes = if let Some(b64) = entry.get("b64_json").and_then(Value::as_str) {
        base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| upstream("openai b64_json", e))?
    } else if let Some(u) = entry.get("url").and_then(Value::as_str) {
        download(http, u, "openai image fetch").await?
    } else {
        return Err(MediaError::Upstream(
            "openai response had neither b64_json nor url".to_owned(),
        ));
    };
    let ext = sniff_image_ext(&bytes);
    Ok(Rendered {
        note: format!(
            "openai/{wire} · {} · {} bytes",
            job.aspect.as_deref().unwrap_or("1:1"),
            bytes.len()
        ),
        bytes,
        ext,
    })
}

/// Submits to fal's queue, polls the status URL, returns the result JSON.
async fn fal_queue(
    http: &reqwest::Client,
    queue_base: &str,
    endpoint: &str,
    key: &str,
    input: &Value,
    limits: Limits,
) -> Result<Value, MediaError> {
    let auth = format!("Key {key}");
    let res = http
        .post(format!("{queue_base}/{endpoint}"))
        .header(reqwest::header::AUTHORIZATION, &auth)
        .json(input)
        .send()
        .await
        .map_err(|e| upstream("fal submit", e))?;
    let submit = json_of(res, "fal submit").await?;
    let request_id = submit
        .get("request_id")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            MediaError::Upstream(format!(
                "fal submit missing request_id: {}",
                truncate(&submit.to_string(), 200)
            ))
        })?;
    let status_url = submit
        .get("status_url")
        .and_then(Value::as_str)
        .map_or_else(
            || format!("{queue_base}/requests/{request_id}/status?logs=0"),
            str::to_owned,
        );
    let result_url = submit
        .get("response_url")
        .and_then(Value::as_str)
        .map_or_else(
            || format!("{queue_base}/requests/{request_id}"),
            str::to_owned,
        );
    let started = Instant::now();
    let mut last = String::new();
    let mut first = true;
    while started.elapsed() < limits.fal_max_poll {
        if !first {
            tokio::time::sleep(limits.fal_poll_interval).await;
        }
        first = false;
        let res = http
            .get(&status_url)
            .header(reqwest::header::AUTHORIZATION, &auth)
            .send()
            .await
            .map_err(|e| upstream("fal poll", e))?;
        let status = json_of(res, "fal poll").await?;
        last = status
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        match last.as_str() {
            "COMPLETED" => {
                let res = http
                    .get(&result_url)
                    .header(reqwest::header::AUTHORIZATION, &auth)
                    .send()
                    .await
                    .map_err(|e| upstream("fal result", e))?;
                return json_of(res, "fal result").await;
            }
            "FAILED" => {
                let err = status
                    .get("error")
                    .and_then(|e| e.get("message").or(Some(e)))
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error");
                return Err(MediaError::Upstream(format!("fal task failed: {err}")));
            }
            _ => {}
        }
    }
    Err(MediaError::Timeout(format!(
        "fal timed out after {}s waiting for COMPLETED (last status: {})",
        started.elapsed().as_secs(),
        if last.is_empty() { "unknown" } else { &last }
    )))
}

fn fal_endpoint(def: ModelDef, job: &Job, creds: &Credentials) -> String {
    if def.wire.is_empty() {
        job.model.clone()
    } else {
        creds.model.clone().unwrap_or_else(|| def.wire.to_owned())
    }
}

fn fal_queue_base(creds: &Credentials) -> String {
    let b = base(creds, Provider::Fal);
    b.strip_prefix("https://fal.run")
        .map_or(b.clone(), |rest| format!("https://queue.fal.run{rest}"))
}

async fn fal_image(
    http: &reqwest::Client,
    def: ModelDef,
    job: &Job,
    creds: &Credentials,
    limits: Limits,
) -> Result<Rendered, MediaError> {
    let key = key(creds, Provider::Fal)?;
    let endpoint = fal_endpoint(def, job, creds);
    let aspect = job.aspect.as_deref().unwrap_or("1:1");
    let mut input = json!({
        "prompt": if job.prompt.is_empty() { "A high-quality image." } else { &job.prompt },
        "num_images": 1,
    });
    if endpoint.starts_with("fal-ai/flux-pro/") {
        input["aspect_ratio"] = json!(aspect);
    } else {
        input["image_size"] = json!(match aspect {
            "16:9" => "landscape_16_9",
            "9:16" => "portrait_16_9",
            "4:3" => "landscape_4_3",
            "3:4" => "portrait_4_3",
            _ => "square_hd",
        });
    }
    let result = fal_queue(http, &fal_queue_base(creds), &endpoint, key, &input, limits).await?;
    let url = result
        .get("images")
        .and_then(|i| i.get(0))
        .and_then(|i| i.get("url"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            MediaError::Upstream(format!(
                "fal image missing images[0].url: {}",
                truncate(&result.to_string(), 200)
            ))
        })?;
    let bytes = download(http, url, "fal image download").await?;
    let ext = sniff_image_ext(&bytes);
    Ok(Rendered {
        note: format!("fal/{endpoint} · {aspect} · {} bytes", bytes.len()),
        bytes,
        ext,
    })
}

async fn fal_video(
    http: &reqwest::Client,
    def: ModelDef,
    job: &Job,
    creds: &Credentials,
    limits: Limits,
) -> Result<Rendered, MediaError> {
    let key = key(creds, Provider::Fal)?;
    let endpoint = fal_endpoint(def, job, creds);
    let aspect = job.aspect.as_deref().unwrap_or("16:9");
    let seconds = job.length.unwrap_or(5.0);
    let mut input = json!({
        "prompt": if job.prompt.is_empty() { "A short cinematic clip." } else { &job.prompt },
        "aspect_ratio": aspect,
    });
    if endpoint == "fal-ai/veo3" || endpoint == "fal-ai/veo2" {
        let closest = [4.0_f64, 6.0, 8.0]
            .into_iter()
            .min_by(|a, b| (a - seconds).abs().total_cmp(&(b - seconds).abs()))
            .unwrap_or(8.0);
        input["duration"] = json!(format!("{closest}s"));
    } else if !endpoint.starts_with("fal-ai/wan-") {
        input["duration"] = json!(seconds);
    }
    let result = fal_queue(http, &fal_queue_base(creds), &endpoint, key, &input, limits).await?;
    let url = result
        .get("video")
        .and_then(|v| v.get("url"))
        .or_else(|| {
            result
                .get("videos")
                .and_then(|v| v.get(0))
                .and_then(|v| v.get("url"))
        })
        .and_then(Value::as_str)
        .ok_or_else(|| {
            MediaError::Upstream(format!(
                "fal video missing video.url: {}",
                truncate(&result.to_string(), 200)
            ))
        })?;
    let bytes = download(http, url, "fal video download").await?;
    Ok(Rendered {
        note: format!("fal/{endpoint} · {aspect} · {} bytes", bytes.len()),
        bytes,
        ext: ".mp4",
    })
}

/// ElevenLabs default voice (Rachel), as in the original.
pub const ELEVENLABS_DEFAULT_VOICE: &str = "21m00Tcm4TlvDq8ikWAM";

async fn elevenlabs(
    http: &reqwest::Client,
    def: ModelDef,
    job: &Job,
    creds: &Credentials,
) -> Result<Rendered, MediaError> {
    let key = key(creds, Provider::ElevenLabs)?;
    let text = job.prompt.trim();
    if text.is_empty() {
        return Err(MediaError::BadRequest(
            "ElevenLabs needs a prompt (the text to voice)".to_owned(),
        ));
    }
    let base = base(creds, Provider::ElevenLabs);
    let wire = creds.model.as_deref().unwrap_or(def.wire);
    let (url, body, label) = if def.id == "elevenlabs-sfx" {
        if text.chars().count() > 450 {
            return Err(MediaError::BadRequest(
                "ElevenLabs SFX prompts are limited to 450 characters".to_owned(),
            ));
        }
        let seconds = job.duration.unwrap_or(5.0).clamp(0.5, 30.0);
        (
            format!("{base}/v1/sound-generation?output_format=mp3_44100_128"),
            json!({ "text": text, "duration_seconds": seconds, "prompt_influence": 0.3, "model_id": wire }),
            format!("{seconds}s"),
        )
    } else {
        let voice = job
            .voice
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or(ELEVENLABS_DEFAULT_VOICE);
        let voice_seg: String = voice
            .bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"-_".contains(&b) {
                    char::from(b).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect();
        (
            format!("{base}/v1/text-to-speech/{voice_seg}?output_format=mp3_44100_128"),
            json!({
                "text": text,
                "model_id": wire,
                "voice_settings": { "stability": 1, "similarity_boost": 1, "style": 0, "speed": 1, "use_speaker_boost": true },
            }),
            voice.to_owned(),
        )
    };
    let res = http
        .post(&url)
        .header("xi-api-key", key)
        .json(&body)
        .send()
        .await
        .map_err(|e| upstream("elevenlabs", e))?;
    let status = res.status();
    if !status.is_success() {
        let text = res.text().await.unwrap_or_default();
        return Err(MediaError::Upstream(format!(
            "elevenlabs {}: {}",
            status.as_u16(),
            truncate(&text, 240)
        )));
    }
    let bytes = res
        .bytes()
        .await
        .map_err(|e| upstream("elevenlabs", e))?
        .to_vec();
    if bytes.is_empty() {
        return Err(MediaError::Upstream(
            "elevenlabs returned zero bytes".to_owned(),
        ));
    }
    Ok(Rendered {
        note: format!("elevenlabs/{wire} · {label} · {} bytes", bytes.len()),
        bytes,
        ext: ".mp3",
    })
}

/// Calls the provider that serves `def` for `job`.
///
/// # Errors
/// A model on the wrong surface, a missing key, upstream failures.
pub async fn render(
    http: &reqwest::Client,
    def: ModelDef,
    job: &Job,
    creds: &Credentials,
    limits: Limits,
) -> Result<Rendered, MediaError> {
    if def.surface != job.surface {
        return Err(MediaError::BadRequest(format!(
            "model {} makes {:?}, not {:?}",
            job.model, def.surface, job.surface
        )));
    }
    match (def.provider, def.surface) {
        (Provider::OpenAi, Surface::Image) => openai_image(http, def, job, creds).await,
        (Provider::Fal, Surface::Image) => fal_image(http, def, job, creds, limits).await,
        (Provider::Fal, Surface::Video) => fal_video(http, def, job, creds, limits).await,
        (Provider::ElevenLabs, Surface::Audio) => elevenlabs(http, def, job, creds).await,
        (p, s) => Err(MediaError::BadRequest(format!(
            "{} cannot make {s:?}",
            p.id()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_and_sizes() {
        assert_eq!(
            find_model("flux-dev-fal", Surface::Image).unwrap().wire,
            "fal-ai/flux/dev"
        );
        let custom = find_model("fal-ai/flux/krea", Surface::Image).unwrap();
        assert_eq!(custom.provider, Provider::Fal);
        assert!(find_model("midjourney", Surface::Image).is_none());
        assert_eq!(openai_size("gpt-image-1", Some("16:9")), "1792x1024");
        assert_eq!(openai_size("dall-e-2", Some("16:9")), "1024x1024");
        assert_eq!(sniff_image_ext(&[0xFF, 0xD8, 0xFF, 0]), ".jpg");
        let creds = Credentials {
            api_key: None,
            base_url: Some("https://fal.run/x".to_owned()),
            model: None,
            source: crate::config::KeySource::Unset,
        };
        assert_eq!(fal_queue_base(&creds), "https://queue.fal.run/x");
    }
}
