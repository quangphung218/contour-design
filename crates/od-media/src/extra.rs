//! The other providers of the original's `media/index.ts` (track C3):
//! Volcengine Ark, xAI Grok, Nano Banana, ImageRouter, OpenRouter, a
//! custom OpenAI-compatible image API, Leonardo.ai, MiniMax, FishAudio,
//! SenseAudio, AIHubMix and OpenAI speech, plus the placeholder renderer
//! (`OD_MEDIA_ALLOW_STUBS`). Request and response shapes follow the
//! original renderers; every call uses the user's key and base URL.

use crate::config::Credentials;
use crate::provider::{
    Job, Limits, MediaError, ModelDef, Provider, Rendered, Surface, base, body_text, download,
    download_client, json_of, key, openai_size, sniff_image_ext, truncate, upstream,
};
use base64::Engine;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const DEFAULT_IMAGE_PROMPT: &str = "A high-quality reference image.";
const DEFAULT_VIDEO_PROMPT: &str = "A short cinematic clip.";
const DEFAULT_SPEECH_TEXT: &str = "This is a test.";

/// The five aspects every provider here knows.
const ASPECTS: [&str; 5] = ["1:1", "16:9", "9:16", "4:3", "3:4"];

fn aspect_or(job: &Job, fallback: &'static str) -> String {
    job.aspect
        .as_deref()
        .filter(|a| ASPECTS.contains(a))
        .unwrap_or(fallback)
        .to_owned()
}

fn prompt_or<'a>(job: &'a Job, fallback: &'a str) -> &'a str {
    if job.prompt.trim().is_empty() {
        fallback
    } else {
        job.prompt.trim()
    }
}

fn wire(def: ModelDef, creds: &Credentials) -> String {
    creds.model.clone().unwrap_or_else(|| def.wire.to_owned())
}

fn b64(s: &str, tag: &str) -> Result<Vec<u8>, MediaError> {
    let raw = s.split_once(',').map_or(s, |(_, b)| b);
    base64::engine::general_purpose::STANDARD
        .decode(raw.trim())
        .map_err(|e| upstream(tag, e))
}

async fn post(
    http: &reqwest::Client,
    url: &str,
    bearer: Option<&str>,
    headers: &[(&str, &str)],
    body: &Value,
    tag: &str,
) -> Result<reqwest::Response, MediaError> {
    let mut req = http.post(url).json(body);
    if let Some(k) = bearer {
        req = req.bearer_auth(k);
    }
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    req.send().await.map_err(|e| upstream(tag, e))
}

async fn get_json(
    http: &reqwest::Client,
    url: &str,
    bearer: &str,
    headers: &[(&str, &str)],
    tag: &str,
) -> Result<Value, MediaError> {
    let mut req = http.get(url).bearer_auth(bearer);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let res = req.send().await.map_err(|e| upstream(tag, e))?;
    json_of(res, tag).await
}

/// Binary body of a successful response; an error status carries its text.
async fn bytes_of(res: reqwest::Response, tag: &str) -> Result<Vec<u8>, MediaError> {
    let status = res.status();
    if !status.is_success() {
        let text = res.text().await.unwrap_or_default();
        return Err(MediaError::Upstream(format!(
            "{tag} {}: {}",
            status.as_u16(),
            truncate(&text, 240)
        )));
    }
    let bytes = res.bytes().await.map_err(|e| upstream(tag, e))?.to_vec();
    if bytes.is_empty() {
        return Err(MediaError::Upstream(format!("{tag} returned zero bytes")));
    }
    Ok(bytes)
}

/// `data[0].b64_json` or `data[0].url` of an OpenAI-shaped answer.
async fn openai_data_bytes(data: &Value, tag: &str) -> Result<Vec<u8>, MediaError> {
    let entry = data
        .get("data")
        .and_then(|d| d.get(0))
        .ok_or_else(|| MediaError::Upstream(format!("{tag} response had no data[0]")))?;
    if let Some(s) = entry
        .get("b64_json")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        return b64(s, tag);
    }
    if let Some(u) = entry
        .get("url")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        return download(u, &format!("{tag} fetch")).await;
    }
    Err(MediaError::Upstream(format!(
        "{tag} response had neither b64_json nor url"
    )))
}

/// `<base>/images/generations` (or `/videos/generations`), whatever of it
/// the user already typed (`normalizeOpenAICompatiblePath`).
#[must_use]
pub fn generation_url(base_url: &str, endpoint: &str) -> String {
    let (head, query) = base_url
        .split_once('?')
        .map_or((base_url, None), |(h, q)| (h, Some(q)));
    let head = head.trim_end_matches('/');
    let generations = format!("/{endpoint}/generations");
    let path = if head.ends_with(&generations) {
        head.to_owned()
    } else if endpoint == "images" && head.ends_with("/images/edits") {
        format!(
            "{}{generations}",
            &head[..head.len() - "/images/edits".len()]
        )
    } else {
        format!("{head}{generations}")
    };
    match query {
        Some(q) => format!("{path}?{q}"),
        None => path,
    }
}

fn speech_url(base_url: &str) -> String {
    let (head, query) = base_url
        .split_once('?')
        .map_or((base_url, None), |(h, q)| (h, Some(q)));
    let path = format!("{}/audio/speech", head.trim_end_matches('/'));
    match query {
        Some(q) => format!("{path}?{q}"),
        None => path,
    }
}

/// Voices OpenAI's speech endpoint takes; anything else is a style note.
const OPENAI_TTS_VOICES: [&str; 11] = [
    "alloy", "ash", "ballad", "coral", "echo", "fable", "onyx", "nova", "sage", "shimmer", "verse",
];

fn origin(url: &str) -> Option<String> {
    let u = reqwest::Url::parse(url).ok()?;
    Some(u.origin().ascii_serialization())
}

/// Downloads an asset URL a provider answered with, refusing one on a
/// loopback or private address unless it is on the provider's own base
/// origin (`assertAndFetchExternalAsset`): a compromised gateway must not
/// make the daemon fetch local services.
async fn download_external(
    url: &str,
    base_url: &str,
    auth: Option<(&str, &str)>,
    tag: &str,
) -> Result<Vec<u8>, MediaError> {
    let parsed = reqwest::Url::parse(url).map_err(|e| upstream(tag, e))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(MediaError::Upstream(format!(
            "{tag}: refusing a {} URL",
            parsed.scheme()
        )));
    }
    let same_origin = origin(url).is_some_and(|o| Some(o) == origin(base_url));
    if !same_origin {
        let host = parsed.host_str().unwrap_or_default().to_owned();
        let port = parsed.port_or_known_default().unwrap_or(443);
        let addrs = tokio::net::lookup_host((host.trim_matches(['[', ']']), port))
            .await
            .map_err(|e| upstream(tag, e))?;
        for addr in addrs {
            if is_internal(addr.ip()) {
                return Err(MediaError::Upstream(format!(
                    "{tag}: refusing an asset URL on an internal address"
                )));
            }
        }
    }
    let mut req = download_client()?.get(url);
    if let (true, Some((k, v))) = (same_origin, auth) {
        req = req.header(k, v);
    }
    let res = req.send().await.map_err(|e| upstream(tag, e))?;
    bytes_of(res, tag).await
}

fn is_internal(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified()
        }
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || (v6.segments()[0] & 0xffc0) == 0xfe80
                || v6.to_ipv4_mapped().is_some_and(|v4| is_internal(v4.into()))
        }
    }
}

fn note(parts: &[&str]) -> String {
    parts.join(" · ")
}

/// Calls the renderer for `def` (a provider not handled in
/// [`crate::provider::render`]).
///
/// # Errors
/// A pair with no renderer, a missing key, upstream failures.
pub async fn render(
    http: &reqwest::Client,
    def: ModelDef,
    job: &Job,
    creds: &Credentials,
    limits: Limits,
) -> Result<Rendered, MediaError> {
    match (def.provider, def.surface) {
        (Provider::OpenAi, Surface::Audio) => openai_speech(http, def, job, creds).await,
        (Provider::Volcengine, Surface::Image) => volcengine_image(http, def, job, creds).await,
        (Provider::Volcengine, Surface::Video) => {
            volcengine_video(http, def, job, creds, limits).await
        }
        (Provider::Grok, Surface::Image) => grok_image(http, def, job, creds).await,
        (Provider::Grok, Surface::Video) => grok_video(http, def, job, creds, limits).await,
        (Provider::Grok, Surface::Audio) => xai_tts(http, def, job, creds).await,
        (Provider::NanoBanana, Surface::Image) => nanobanana(http, def, job, creds).await,
        (Provider::ImageRouter, Surface::Image | Surface::Video) => {
            imagerouter(http, def, job, creds).await
        }
        (Provider::OpenRouter, Surface::Image) => openrouter_image(http, def, job, creds).await,
        (Provider::OpenRouter, Surface::Video) => {
            openrouter_video(http, def, job, creds, limits).await
        }
        (Provider::CustomImage, Surface::Image) => custom_image(http, def, job, creds).await,
        (Provider::Leonardo, Surface::Image) => leonardo(http, def, job, creds, limits).await,
        (Provider::MiniMax, Surface::Audio) => minimax_tts(http, def, job, creds).await,
        (Provider::MiniMax, Surface::Image) => minimax_image(http, def, job, creds).await,
        (Provider::FishAudio, Surface::Audio) => fishaudio(http, def, job, creds).await,
        (Provider::SenseAudio, Surface::Audio) => senseaudio_tts(http, def, job, creds).await,
        (Provider::SenseAudio, Surface::Image) => senseaudio_image(http, def, job, creds).await,
        (Provider::AiHubMix, Surface::Image) => aihubmix_image(http, def, job, creds).await,
        (Provider::AiHubMix, Surface::Audio) => aihubmix_tts(http, def, job, creds).await,
        (Provider::AiHubMix, Surface::Video) => aihubmix_video(http, def, job, creds, limits).await,
        (p, s) => Err(MediaError::BadRequest(format!(
            "{} cannot make {s:?}",
            p.id()
        ))),
    }
}

async fn openai_speech(
    http: &reqwest::Client,
    def: ModelDef,
    job: &Job,
    creds: &Credentials,
) -> Result<Rendered, MediaError> {
    let k = key(creds, Provider::OpenAi)?;
    let wire = wire(def, creds);
    let (voice, instructions) = match job
        .voice
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        Some(v) if OPENAI_TTS_VOICES.contains(&v) => (v.to_owned(), None),
        Some(style) => ("alloy".to_owned(), Some(style.to_owned())),
        None => ("alloy".to_owned(), None),
    };
    let mut body = json!({
        "model": wire,
        "input": prompt_or(job, DEFAULT_SPEECH_TEXT),
        "voice": voice,
        "response_format": "mp3",
    });
    if let (Some(style), "gpt-4o-mini-tts") = (&instructions, def.id) {
        body["instructions"] = json!(style);
    }
    let url = speech_url(&base(creds, Provider::OpenAi));
    let res = post(http, &url, Some(k), &[], &body, "openai speech").await?;
    let bytes = bytes_of(res, "openai speech").await?;
    let size = format!("{} bytes", bytes.len());
    let mut bits = vec![format!("openai/{wire}"), voice];
    if instructions.is_some() {
        bits.push("styled".to_owned());
    }
    bits.extend(["mp3".to_owned(), size]);
    Ok(Rendered {
        note: bits.join(" · "),
        bytes,
        ext: ".mp3",
    })
}

async fn volcengine_image(
    http: &reqwest::Client,
    def: ModelDef,
    job: &Job,
    creds: &Credentials,
) -> Result<Rendered, MediaError> {
    let k = key(creds, Provider::Volcengine)?;
    let body = json!({
        "model": def.wire,
        "prompt": prompt_or(job, DEFAULT_IMAGE_PROMPT),
        "response_format": "b64_json",
        "size": openai_size(def.id, job.aspect.as_deref()),
    });
    let url = format!("{}/images/generations", base(creds, Provider::Volcengine));
    let res = post(http, &url, Some(k), &[], &body, "volcengine image").await?;
    let data = json_of(res, "volcengine image").await?;
    let bytes = openai_data_bytes(&data, "volcengine image").await?;
    Ok(Rendered {
        note: note(&[
            &format!("volcengine/{}", def.wire),
            job.aspect.as_deref().unwrap_or("1:1"),
            &format!("{} bytes", bytes.len()),
        ]),
        bytes,
        ext: ".png",
    })
}

/// Seedance reads `--resolution`, `--duration` and `--ratio` from the
/// prompt; ones the user already wrote win.
fn seedance_prompt(prompt: &str, ratio: &str, seconds: f64) -> String {
    let mut text = prompt.to_owned();
    for (flag, value) in [
        ("--resolution", "720p".to_owned()),
        ("--duration", format!("{seconds}")),
        ("--ratio", ratio.to_owned()),
    ] {
        if !prompt.contains(flag) {
            text.push_str(&format!(" {flag} {value}"));
        }
    }
    text
}

/// Polls `url` every `interval` until `step` returns a result or an
/// error, or `cap` passes (`timing` = `(interval, cap)`).
async fn poll_until(
    http: &reqwest::Client,
    url: &str,
    bearer: &str,
    headers: &[(&str, &str)],
    (interval, cap): (Duration, Duration),
    tag: &str,
    mut step: impl FnMut(&Value) -> Result<Option<Value>, MediaError>,
) -> Result<Value, MediaError> {
    let started = Instant::now();
    let mut last = Value::Null;
    while started.elapsed() < cap {
        tokio::time::sleep(interval).await;
        last = get_json(http, url, bearer, headers, &format!("{tag} poll")).await?;
        if let Some(done) = step(&last)? {
            return Ok(done);
        }
    }
    let status = ["status", "state"]
        .iter()
        .find_map(|k| last.get(*k).and_then(Value::as_str))
        .unwrap_or("unknown")
        .to_owned();
    Err(MediaError::Timeout(format!(
        "{tag} did not finish in {} s (last status: {status})",
        cap.as_secs()
    )))
}

async fn volcengine_video(
    http: &reqwest::Client,
    def: ModelDef,
    job: &Job,
    creds: &Credentials,
    limits: Limits,
) -> Result<Rendered, MediaError> {
    let k = key(creds, Provider::Volcengine)?;
    let base = base(creds, Provider::Volcengine);
    let ratio = aspect_or(job, "16:9");
    let seconds = job.length.unwrap_or(5.0);
    let text = seedance_prompt(prompt_or(job, DEFAULT_VIDEO_PROMPT), &ratio, seconds);
    let body = json!({"model": def.wire, "content": [{"type": "text", "text": text}]});
    let tasks = format!("{base}/contents/generations/tasks");
    let res = post(http, &tasks, Some(k), &[], &body, "volcengine task create").await?;
    let task = json_of(res, "volcengine task create").await?;
    let id = task
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| MediaError::Upstream("volcengine task response missing id".to_owned()))?
        .to_owned();
    let done = poll_until(
        http,
        &format!("{tasks}/{id}"),
        k,
        &[],
        (limits.poll_interval, Duration::from_secs(12 * 60)),
        "volcengine task",
        |p| match p.get("status").and_then(Value::as_str) {
            Some("succeeded") => Ok(Some(p.clone())),
            Some(s @ ("failed" | "cancelled")) => Err(MediaError::Upstream(format!(
                "volcengine task {s}: {}",
                p.pointer("/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or(s)
            ))),
            _ => Ok(None),
        },
    )
    .await?;
    let url = done
        .pointer("/content/video_url")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            MediaError::Upstream("volcengine task finished without video_url".to_owned())
        })?;
    let bytes = download(url, "volcengine video fetch").await?;
    Ok(Rendered {
        note: note(&[
            &format!("volcengine/{}", def.wire),
            &ratio,
            &format!("{seconds}s"),
            &format!("{} bytes", bytes.len()),
        ]),
        bytes,
        ext: ".mp4",
    })
}

async fn grok_image(
    http: &reqwest::Client,
    def: ModelDef,
    job: &Job,
    creds: &Credentials,
) -> Result<Rendered, MediaError> {
    let k = key(creds, Provider::Grok)?;
    let aspect = aspect_or(job, "16:9");
    let body = json!({
        "model": def.wire,
        "prompt": prompt_or(job, DEFAULT_IMAGE_PROMPT),
        "n": 1,
        "aspect_ratio": aspect,
        "response_format": "b64_json",
    });
    let url = format!("{}/images/generations", base(creds, Provider::Grok));
    let res = post(http, &url, Some(k), &[], &body, "grok image").await?;
    let data = json_of(res, "grok image").await?;
    let bytes = openai_data_bytes(&data, "grok image").await?;
    let ext = sniff_image_ext(&bytes);
    Ok(Rendered {
        note: note(&[
            &format!("grok/{}", def.wire),
            &aspect,
            &format!("{} bytes", bytes.len()),
        ]),
        bytes,
        ext,
    })
}

async fn grok_video(
    http: &reqwest::Client,
    def: ModelDef,
    job: &Job,
    creds: &Credentials,
    limits: Limits,
) -> Result<Rendered, MediaError> {
    let k = key(creds, Provider::Grok)?;
    let base = base(creds, Provider::Grok);
    let seconds = job.length.unwrap_or(5.0).clamp(1.0, 15.0);
    let aspect = aspect_or(job, "16:9");
    let body = json!({
        "model": def.wire,
        "prompt": prompt_or(job, DEFAULT_VIDEO_PROMPT),
        "duration": seconds,
        "aspect_ratio": aspect,
        "resolution": "720p",
    });
    let res = post(
        http,
        &format!("{base}/videos/generations"),
        Some(k),
        &[],
        &body,
        "grok video submit",
    )
    .await?;
    let submit = json_of(res, "grok video submit").await?;
    let inline = submit
        .pointer("/video/url")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let url = match inline {
        Some(u) => u,
        None => {
            let id = submit
                .get("id")
                .or_else(|| submit.get("request_id"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    MediaError::Upstream(format!(
                        "grok video submit returned no inline video and no request_id to poll (status={})",
                        submit.get("status").and_then(Value::as_str).unwrap_or("unknown")
                    ))
                })?
                .to_owned();
            let done = poll_until(
                http,
                &format!("{base}/videos/{id}"),
                k,
                &[],
                (limits.poll_interval, Duration::from_secs(8 * 60)),
                "grok video",
                |p| match p.get("status").and_then(Value::as_str) {
                    Some("done" | "succeeded") => Ok(Some(p.clone())),
                    Some(s @ ("failed" | "expired" | "error")) => {
                        Err(MediaError::Upstream(format!(
                            "grok video {s}: {}",
                            p.pointer("/error/message")
                                .or_else(|| p.get("error"))
                                .and_then(Value::as_str)
                                .unwrap_or(s)
                        )))
                    }
                    _ => Ok(None),
                },
            )
            .await?;
            done.pointer("/video/url")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    MediaError::Upstream("grok video finished without video.url".to_owned())
                })?
                .to_owned()
        }
    };
    let bytes = download(&url, "grok video fetch").await?;
    Ok(Rendered {
        note: note(&[
            &format!("grok/{}", def.wire),
            &aspect,
            &format!("{seconds}s"),
            &format!("{} bytes", bytes.len()),
        ]),
        bytes,
        ext: ".mp4",
    })
}

async fn xai_tts(
    http: &reqwest::Client,
    def: ModelDef,
    job: &Job,
    creds: &Credentials,
) -> Result<Rendered, MediaError> {
    let k = key(creds, Provider::Grok)?;
    let voice = job
        .voice
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("eve")
        .to_owned();
    let body =
        json!({"text": prompt_or(job, DEFAULT_SPEECH_TEXT), "voice_id": voice, "language": "en"});
    let url = format!("{}/tts", base(creds, Provider::Grok));
    let res = post(http, &url, Some(k), &[], &body, "xai tts").await?;
    let bytes = bytes_of(res, "xai tts").await?;
    Ok(Rendered {
        note: note(&[
            &format!("xai/{}", def.wire),
            &format!("voice={voice}"),
            "en",
            &format!("{} bytes", bytes.len()),
        ]),
        bytes,
        ext: ".mp3",
    })
}

/// Inline image bytes of a Gemini `generateContent` answer.
fn gemini_inline_bytes(data: &Value, tag: &str) -> Result<Vec<u8>, MediaError> {
    let found = data
        .get("candidates")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|c| {
            c.pointer("/content/parts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .find_map(|p| {
            p.pointer("/inlineData/data")
                .or_else(|| p.pointer("/inline_data/data"))
                .and_then(Value::as_str)
                .filter(|d| !d.is_empty())
        });
    match found {
        Some(d) => b64(d, tag),
        None => Err(MediaError::Upstream(format!(
            "{tag} response missing candidates[].content.parts[].inlineData.data"
        ))),
    }
}

async fn nanobanana(
    http: &reqwest::Client,
    def: ModelDef,
    job: &Job,
    creds: &Credentials,
) -> Result<Rendered, MediaError> {
    let k = key(creds, Provider::NanoBanana)?;
    let base = base(creds, Provider::NanoBanana);
    let wire = wire(def, creds);
    let aspect = aspect_or(job, "1:1");
    let body = json!({
        "contents": [{"parts": [{"text": prompt_or(job, DEFAULT_IMAGE_PROMPT)}]}],
        "generationConfig": {
            "responseModalities": ["IMAGE"],
            "imageConfig": {"aspectRatio": aspect, "imageSize": "1K"},
        },
    });
    let url = format!("{base}/v1beta/models/{wire}:generateContent");
    // Google's own host takes `x-goog-api-key`; gateways take a bearer.
    let official = reqwest::Url::parse(&base)
        .is_ok_and(|u| u.host_str() == Some("generativelanguage.googleapis.com"));
    let res = if official {
        post(
            http,
            &url,
            None,
            &[("x-goog-api-key", k)],
            &body,
            "nano-banana image",
        )
        .await?
    } else {
        post(http, &url, Some(k), &[], &body, "nano-banana image").await?
    };
    let data = json_of(res, "nano-banana image").await?;
    let bytes = gemini_inline_bytes(&data, "nano-banana image")?;
    let ext = sniff_image_ext(&bytes);
    Ok(Rendered {
        note: note(&[
            &format!("nano-banana/{wire}"),
            &aspect,
            "1K",
            &format!("{} bytes", bytes.len()),
        ]),
        bytes,
        ext,
    })
}

fn imagerouter_size(aspect: Option<&str>, surface: Surface) -> &'static str {
    match aspect {
        Some("16:9") => "1024x576",
        Some("9:16") => "576x1024",
        Some("4:3") => "1024x768",
        Some("3:4") => "768x1024",
        Some("1:1") => "1024x1024",
        _ if surface == Surface::Video => "1024x576",
        _ => "1024x1024",
    }
}

async fn imagerouter(
    http: &reqwest::Client,
    def: ModelDef,
    job: &Job,
    creds: &Credentials,
) -> Result<Rendered, MediaError> {
    let k = key(creds, Provider::ImageRouter)?;
    let base = base(creds, Provider::ImageRouter);
    let wire = wire(def, creds);
    let size = imagerouter_size(job.aspect.as_deref(), def.surface);
    let (body, url, tag) = if def.surface == Surface::Video {
        let seconds = job.length.map_or_else(|| json!("auto"), |s| json!(s));
        (
            json!({
                "prompt": prompt_or(job, DEFAULT_VIDEO_PROMPT),
                "model": wire,
                "size": size,
                "seconds": seconds,
                "response_format": "b64_json",
            }),
            generation_url(&base, "videos"),
            "imagerouter video",
        )
    } else {
        (
            json!({
                "prompt": prompt_or(job, DEFAULT_IMAGE_PROMPT),
                "model": wire,
                "quality": "auto",
                "size": size,
                "response_format": "b64_json",
                "output_format": "png",
            }),
            generation_url(&base, "images"),
            "imagerouter image",
        )
    };
    let res = post(http, &url, Some(k), &[], &body, tag).await?;
    let data = json_of(res, tag).await?;
    let bytes = openai_data_bytes(&data, tag).await?;
    let ext = if def.surface == Surface::Video {
        ".mp4"
    } else {
        sniff_image_ext(&bytes)
    };
    Ok(Rendered {
        note: note(&[
            &format!("imagerouter/{wire}"),
            size,
            &format!("{} bytes", bytes.len()),
        ]),
        bytes,
        ext,
    })
}

const OPENROUTER_HEADERS: [(&str, &str); 2] = [
    ("HTTP-Referer", "https://opendesign.dev"),
    ("X-Title", "Contour"),
];

fn openrouter_slug(def: ModelDef, creds: &Credentials) -> String {
    let resolved = wire(def, creds);
    resolved
        .strip_prefix("openrouter/")
        .unwrap_or(&resolved)
        .to_owned()
}

async fn openrouter_image(
    http: &reqwest::Client,
    def: ModelDef,
    job: &Job,
    creds: &Credentials,
) -> Result<Rendered, MediaError> {
    let k = key(creds, Provider::OpenRouter)?;
    let slug = openrouter_slug(def, creds);
    let aspect = aspect_or(job, "16:9");
    let modalities = if slug.contains("gemini") {
        json!(["image", "text"])
    } else {
        json!(["image"])
    };
    let body = json!({
        "model": slug,
        "messages": [{"role": "user", "content": prompt_or(job, DEFAULT_IMAGE_PROMPT)}],
        "modalities": modalities,
        "stream": false,
        "image_config": {"aspect_ratio": aspect, "image_size": "1K"},
    });
    let url = format!("{}/chat/completions", base(creds, Provider::OpenRouter));
    let res = post(
        http,
        &url,
        Some(k),
        &OPENROUTER_HEADERS,
        &body,
        "openrouter image",
    )
    .await?;
    let text = body_text(res, "openrouter image").await?;
    let data: Value = serde_json::from_str(&text).map_err(|_| {
        MediaError::Upstream(format!(
            "openrouter image non-JSON response: {}",
            truncate(&text, 200)
        ))
    })?;
    let image = data
        .pointer("/choices/0/message/images/0/image_url/url")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            MediaError::Upstream(format!(
                "openrouter image response contained no images for model {slug}: {}",
                truncate(&text, 200)
            ))
        })?;
    let bytes = if image.starts_with("data:") {
        b64(image, "openrouter image")?
    } else if image.starts_with("http") {
        download(image, "openrouter image download").await?
    } else {
        b64(image, "openrouter image")?
    };
    let ext = sniff_image_ext(&bytes);
    Ok(Rendered {
        note: note(&[
            &format!("openrouter/{slug}"),
            &aspect,
            &format!("{} bytes", bytes.len()),
        ]),
        bytes,
        ext,
    })
}

async fn openrouter_video(
    http: &reqwest::Client,
    def: ModelDef,
    job: &Job,
    creds: &Credentials,
    limits: Limits,
) -> Result<Rendered, MediaError> {
    let k = key(creds, Provider::OpenRouter)?;
    let base = base(creds, Provider::OpenRouter);
    let after = openrouter_slug(def, creds);
    // `bytedance/seedance-2.0:1080p` → model + resolution.
    let (slug, resolution) = match after.rsplit_once(':') {
        Some((m, r))
            if r.ends_with('p') && r[..r.len() - 1].bytes().all(|b| b.is_ascii_digit()) =>
        {
            (m.to_owned(), r.to_owned())
        }
        _ => (after.clone(), "720p".to_owned()),
    };
    let aspect = aspect_or(job, "16:9");
    let body = json!({
        "model": slug,
        "prompt": prompt_or(job, DEFAULT_VIDEO_PROMPT),
        "aspect_ratio": aspect,
        "resolution": resolution,
        "duration": job.length.unwrap_or(5.0),
    });
    let res = post(
        http,
        &format!("{base}/videos"),
        Some(k),
        &OPENROUTER_HEADERS,
        &body,
        "openrouter video submit",
    )
    .await?;
    let submit = json_of(res, "openrouter video submit").await?;
    let polling = submit
        .get("polling_url")
        .and_then(Value::as_str)
        .filter(|_| submit.get("id").is_some())
        .ok_or_else(|| {
            MediaError::Upstream(format!(
                "openrouter video submit returned no job id or polling_url: {}",
                truncate(&submit.to_string(), 200)
            ))
        })?
        .to_owned();
    let done = poll_until(
        http,
        &polling,
        k,
        &OPENROUTER_HEADERS,
        (limits.poll_interval, Duration::from_secs(30 * 60)),
        "openrouter video",
        |p| match p.get("status").and_then(Value::as_str) {
            Some("completed") => Ok(Some(p.clone())),
            Some(s @ ("failed" | "expired" | "cancelled")) => {
                let reason = p
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| {
                        p.get("error")
                            .map(|e| e.as_str().map_or_else(|| e.to_string(), str::to_owned))
                    })
                    .unwrap_or_else(|| s.to_owned());
                Err(MediaError::Upstream(format!(
                    "openrouter job {s}: {reason}"
                )))
            }
            _ => Ok(None),
        },
    )
    .await?;
    let url = done
        .pointer("/unsigned_urls/0")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            MediaError::Upstream("openrouter job completed without unsigned_urls".to_owned())
        })?;
    // The key goes only to openrouter.ai itself, never to a CDN.
    let to_openrouter =
        reqwest::Url::parse(url).is_ok_and(|u| u.host_str() == Some("openrouter.ai"));
    let bytes = if to_openrouter {
        let res = download_client()?
            .get(url)
            .bearer_auth(k)
            .send()
            .await
            .map_err(|e| upstream("openrouter video download", e))?;
        bytes_of(res, "openrouter video download").await?
    } else {
        download(url, "openrouter video download").await?
    };
    Ok(Rendered {
        note: note(&[
            &format!("openrouter/{slug}"),
            &aspect,
            &format!("{} bytes", bytes.len()),
        ]),
        bytes,
        ext: ".mp4",
    })
}

/// Whether the Custom Image API takes over an OpenAI model: it has a base
/// URL and its model names that model (`customImageOverridesOpenAIModel`).
#[must_use]
pub fn custom_image_overrides(custom: &Credentials, def: ModelDef) -> bool {
    let model = custom.model.as_deref().map(str::trim).unwrap_or_default();
    custom
        .base_url
        .as_deref()
        .is_some_and(|b| !b.trim().is_empty())
        && !model.is_empty()
        && (model == def.id || model == def.wire)
}

/// Custom OpenAI-compatible images endpoint; the key is optional (local
/// servers often take none).
pub async fn custom_image(
    http: &reqwest::Client,
    def: ModelDef,
    job: &Job,
    creds: &Credentials,
) -> Result<Rendered, MediaError> {
    let base = creds.base_url.as_deref().map(str::trim).unwrap_or_default();
    if base.is_empty() {
        return Err(MediaError::BadRequest(
            "Custom Image API base URL required: configure an OpenAI-compatible /v1/images/generations endpoint in Settings → Media".to_owned(),
        ));
    }
    let wire = creds
        .model
        .clone()
        .or_else(|| (def.wire != "custom-image").then(|| def.wire.to_owned()))
        .unwrap_or_default();
    if wire.trim().is_empty() {
        return Err(MediaError::BadRequest(
            "Custom Image API model required: configure the provider model in Settings → Media"
                .to_owned(),
        ));
    }
    let size = openai_size("gpt-image-1", job.aspect.as_deref());
    let body = json!({
        "prompt": prompt_or(job, DEFAULT_IMAGE_PROMPT),
        "model": wire,
        "n": 1,
        "size": size,
    });
    let url = generation_url(base, "images");
    let res = post(
        http,
        &url,
        creds.api_key.as_deref(),
        &[],
        &body,
        "custom image",
    )
    .await?;
    let data = json_of(res, "custom image").await?;
    let bytes = openai_data_bytes(&data, "custom image").await?;
    let ext = sniff_image_ext(&bytes);
    Ok(Rendered {
        note: note(&[
            &format!("custom-image/{wire}"),
            size,
            &format!("{} bytes", bytes.len()),
        ]),
        bytes,
        ext,
    })
}

async fn leonardo(
    http: &reqwest::Client,
    def: ModelDef,
    job: &Job,
    creds: &Credentials,
    limits: Limits,
) -> Result<Rendered, MediaError> {
    let k = key(creds, Provider::Leonardo)?;
    let base = base(creds, Provider::Leonardo);
    let (width, height) = match job.aspect.as_deref() {
        Some("16:9") => (1344, 768),
        Some("9:16") => (768, 1344),
        Some("4:3") => (1152, 896),
        Some("3:4") => (896, 1152),
        _ => (1024, 1024),
    };
    let mut body = json!({
        "prompt": prompt_or(job, DEFAULT_IMAGE_PROMPT),
        "modelId": def.wire,
        "width": width,
        "height": height,
        "num_images": 1,
    });
    if matches!(
        def.id,
        "leonardo-phoenix" | "leonardo-flux-dev" | "leonardo-flux-schnell"
    ) {
        body["contrast"] = json!(3.5);
    }
    let res = post(
        http,
        &format!("{base}/generations"),
        Some(k),
        &[],
        &body,
        "leonardo.ai submit",
    )
    .await?;
    let submit = json_of(res, "leonardo.ai submit").await?;
    let id = submit
        .pointer("/sdGenerationJob/generationId")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            MediaError::Upstream("leonardo.ai response missing generationId".to_owned())
        })?
        .to_owned();
    let done = poll_until(
        http,
        &format!("{base}/generations/{id}"),
        k,
        &[],
        (limits.poll_interval / 2, Duration::from_secs(120)),
        "leonardo.ai generation",
        |p| match p
            .pointer("/generations_by_pk/status")
            .and_then(Value::as_str)
        {
            Some("COMPLETE")
                if p.pointer("/generations_by_pk/generated_images/0/url")
                    .is_some() =>
            {
                Ok(Some(p.clone()))
            }
            Some("FAILED") => Err(MediaError::Upstream(
                "leonardo.ai generation failed".to_owned(),
            )),
            _ => Ok(None),
        },
    )
    .await?;
    let url = done
        .pointer("/generations_by_pk/generated_images/0/url")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let bytes = download(url, "leonardo.ai image fetch").await?;
    let ext = sniff_image_ext(&bytes);
    Ok(Rendered {
        note: note(&[
            &format!("leonardo.ai/{}", def.id),
            job.aspect.as_deref().unwrap_or("1:1"),
            &format!("{} bytes", bytes.len()),
        ]),
        bytes,
        ext,
    })
}

/// MiniMax and SenseAudio answer 200 with `base_resp.status_code != 0` on
/// errors.
fn check_base_resp(data: &Value, tag: &str) -> Result<(), MediaError> {
    match data
        .pointer("/base_resp/status_code")
        .and_then(Value::as_i64)
    {
        Some(0) | None => Ok(()),
        Some(code) => Err(MediaError::Upstream(format!(
            "{tag} api error {code}: {}",
            data.pointer("/base_resp/status_msg")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        ))),
    }
}

/// Hex audio of a `t2a_v2` answer (MiniMax, SenseAudio) and its length.
fn t2a_audio(data: &Value, tag: &str) -> Result<(Vec<u8>, String), MediaError> {
    check_base_resp(data, tag)?;
    let audio = data
        .pointer("/data/audio")
        .and_then(Value::as_str)
        .filter(|a| !a.is_empty())
        .ok_or_else(|| MediaError::Upstream(format!("{tag} response missing data.audio")))?;
    let bytes = hex::decode(audio).map_err(|e| upstream(tag, e))?;
    if bytes.is_empty() {
        return Err(MediaError::Upstream(format!("{tag} decoded zero bytes")));
    }
    let seconds = data
        .pointer("/extra_info/audio_length")
        .and_then(Value::as_f64)
        .map_or_else(
            || "?".to_owned(),
            |ms| format!("{}", (ms / 100.0).round() / 10.0),
        );
    Ok((bytes, seconds))
}

fn voice_or(job: &Job, fallback: &str) -> String {
    job.voice
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or(fallback)
        .to_owned()
}

async fn minimax_tts(
    http: &reqwest::Client,
    def: ModelDef,
    job: &Job,
    creds: &Credentials,
) -> Result<Rendered, MediaError> {
    let k = key(creds, Provider::MiniMax)?;
    let voice = voice_or(job, "male-qn-qingse");
    let body = json!({
        "model": def.wire,
        "text": prompt_or(job, DEFAULT_SPEECH_TEXT),
        "stream": false,
        "voice_setting": {"voice_id": voice, "speed": 1.0, "vol": 1.0, "pitch": 0},
        "audio_setting": {"sample_rate": 32000, "format": "mp3"},
    });
    let url = format!("{}/t2a_v2", base(creds, Provider::MiniMax));
    let res = post(http, &url, Some(k), &[], &body, "minimax tts").await?;
    let data = json_of(res, "minimax tts").await?;
    let (bytes, seconds) = t2a_audio(&data, "minimax tts")?;
    Ok(Rendered {
        note: note(&[
            &format!("minimax/{}", def.wire),
            &voice,
            &format!("{seconds}s"),
            &format!("{} bytes", bytes.len()),
        ]),
        bytes,
        ext: ".mp3",
    })
}

/// Where MiniMax image generation lives; its key works on another host
/// than the TTS one (`OD_MINIMAX_IMAGE_BASE_URL` overrides it).
pub const MINIMAX_IMAGE_BASE_URL: &str = "https://api.minimax.io";

async fn minimax_image(
    http: &reqwest::Client,
    def: ModelDef,
    job: &Job,
    creds: &Credentials,
) -> Result<Rendered, MediaError> {
    let k = key(creds, Provider::MiniMax)?;
    let wire = wire(def, creds);
    let mut body = json!({
        "model": wire,
        "prompt": prompt_or(job, DEFAULT_IMAGE_PROMPT),
        "response_format": "base64",
        "n": 1,
    });
    let aspect = job
        .aspect
        .as_deref()
        .filter(|a| ASPECTS.contains(a) || matches!(*a, "3:2" | "2:3" | "21:9"));
    if let Some(a) = aspect {
        body["aspect_ratio"] = json!(a);
    }
    // `creds.base_url` here is the image host (see `crate::generate`).
    let base = creds
        .base_url
        .as_deref()
        .unwrap_or(MINIMAX_IMAGE_BASE_URL)
        .trim_end_matches('/');
    let res = post(
        http,
        &format!("{base}/v1/image_generation"),
        Some(k),
        &[],
        &body,
        "minimax image",
    )
    .await?;
    let data = json_of(res, "minimax image").await?;
    check_base_resp(&data, "minimax image")?;
    let image = data
        .pointer("/data/image_base64/0")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            MediaError::Upstream("minimax image response missing data.image_base64[0]".to_owned())
        })?;
    let bytes = b64(image, "minimax image")?;
    let ext = sniff_image_ext(&bytes);
    Ok(Rendered {
        note: note(&[
            &format!("minimax/{wire}"),
            aspect.unwrap_or("1:1"),
            &format!("{} bytes", bytes.len()),
        ]),
        bytes,
        ext,
    })
}

async fn fishaudio(
    http: &reqwest::Client,
    def: ModelDef,
    job: &Job,
    creds: &Credentials,
) -> Result<Rendered, MediaError> {
    let k = key(creds, Provider::FishAudio)?;
    let mut body = json!({
        "text": prompt_or(job, DEFAULT_SPEECH_TEXT),
        "format": "mp3",
        "mp3_bitrate": 128,
        "model": def.wire,
        "normalize": true,
        "latency": "normal",
    });
    if let Some(v) = job
        .voice
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        body["reference_id"] = json!(v);
    }
    let url = format!("{}/v1/tts", base(creds, Provider::FishAudio));
    let res = post(http, &url, Some(k), &[], &body, "fishaudio tts").await?;
    let bytes = bytes_of(res, "fishaudio tts").await?;
    Ok(Rendered {
        note: note(&[
            &format!("fishaudio/{}", def.wire),
            &format!("{} bytes", bytes.len()),
        ]),
        bytes,
        ext: ".mp3",
    })
}

async fn senseaudio_tts(
    http: &reqwest::Client,
    def: ModelDef,
    job: &Job,
    creds: &Credentials,
) -> Result<Rendered, MediaError> {
    let k = key(creds, Provider::SenseAudio)?;
    let voice = voice_or(job, "female_0033_b");
    let body = json!({
        "model": def.wire,
        "text": prompt_or(job, DEFAULT_SPEECH_TEXT),
        "stream": false,
        "voice_setting": {"voice_id": voice, "speed": 1, "vol": 1, "pitch": 0},
        "audio_setting": {"format": "mp3", "sample_rate": 32000, "bitrate": 128_000, "channel": 2},
    });
    let url = format!("{}/v1/t2a_v2", base(creds, Provider::SenseAudio));
    let res = post(http, &url, Some(k), &[], &body, "senseaudio tts").await?;
    let data = json_of(res, "senseaudio tts").await?;
    let (bytes, seconds) = t2a_audio(&data, "senseaudio tts")?;
    Ok(Rendered {
        note: note(&[
            &format!("senseaudio/{}", def.wire),
            &voice,
            &format!("{seconds}s"),
            &format!("{} bytes", bytes.len()),
        ]),
        bytes,
        ext: ".mp3",
    })
}

async fn senseaudio_image(
    http: &reqwest::Client,
    def: ModelDef,
    job: &Job,
    creds: &Credentials,
) -> Result<Rendered, MediaError> {
    let k = key(creds, Provider::SenseAudio)?;
    let base = base(creds, Provider::SenseAudio);
    let prompt: String = prompt_or(job, DEFAULT_IMAGE_PROMPT)
        .chars()
        .take(2000)
        .collect();
    let size = match job.aspect.as_deref() {
        Some("16:9") => "1280x720",
        Some("9:16") => "720x1280",
        Some("4:3") => "1024x768",
        Some("3:4") => "768x1024",
        _ => "1024x1024",
    };
    let body = json!({"model": def.wire, "prompt": prompt, "size": size});
    let res = post(
        http,
        &format!("{base}/v1/image/sync"),
        Some(k),
        &[],
        &body,
        "senseaudio image",
    )
    .await?;
    let data = json_of(res, "senseaudio image").await?;
    check_base_resp(&data, "senseaudio image")?;
    if let Some(e) = data
        .get("error_message")
        .and_then(Value::as_str)
        .filter(|e| !e.is_empty())
    {
        return Err(MediaError::Upstream(format!(
            "senseaudio image api error: {e}"
        )));
    }
    let url = data
        .get("url")
        .and_then(Value::as_str)
        .filter(|u| !u.is_empty())
        .ok_or_else(|| MediaError::Upstream("senseaudio image response missing url".to_owned()))?;
    let bytes = download_external(url, &base, None, "senseaudio image fetch").await?;
    Ok(Rendered {
        note: note(&[
            &format!("senseaudio/{}", def.wire),
            size,
            &format!("{} bytes", bytes.len()),
        ]),
        bytes,
        ext: ".png",
    })
}

/// AIHubMix's app attribution header, sent with every call.
const AIHUBMIX_APP_CODE: (&str, &str) = ("APP-Code", "DMCY9912");

/// `aihubmix-gpt-image-1` → `gpt-image-1`; other ids lose the prefix.
fn aihubmix_wire(def: ModelDef, creds: &Credentials) -> String {
    let id = creds.model.clone().unwrap_or_else(|| def.wire.to_owned());
    id.strip_prefix("aihubmix-")
        .map_or(id.clone(), str::to_owned)
}

/// Gemini and Imagen models go through AIHubMix's Gemini-native route.
fn aihubmix_is_gemini(model: &str) -> bool {
    let m = model.trim().to_ascii_lowercase();
    (m.starts_with("gemini") || m.starts_with("imagen"))
        && !m.ends_with("-nothink")
        && !m.ends_with("-search")
        && !m.contains("embedding")
}

async fn aihubmix_image(
    http: &reqwest::Client,
    def: ModelDef,
    job: &Job,
    creds: &Credentials,
) -> Result<Rendered, MediaError> {
    let k = key(creds, Provider::AiHubMix)?;
    let base = base(creds, Provider::AiHubMix);
    let wire = aihubmix_wire(def, creds);
    let aspect = job.aspect.clone().unwrap_or_else(|| "1:1".to_owned());
    if aihubmix_is_gemini(&wire) {
        let origin = origin(&base).unwrap_or_else(|| "https://aihubmix.com".to_owned());
        let url = format!("{origin}/gemini/v1beta/models/{wire}:generateContent");
        let body = json!({
            "contents": [{"role": "user", "parts": [{"text": prompt_or(job, DEFAULT_IMAGE_PROMPT)}]}],
            "generationConfig": {"responseModalities": ["TEXT", "IMAGE"], "imageConfig": {"aspectRatio": aspect}},
        });
        let res = post(
            http,
            &url,
            None,
            &[("x-goog-api-key", k), AIHUBMIX_APP_CODE],
            &body,
            "aihubmix image (gemini)",
        )
        .await?;
        let data = json_of(res, "aihubmix image (gemini)").await?;
        let bytes = gemini_inline_bytes(&data, "aihubmix image (gemini)")?;
        return Ok(Rendered {
            note: format!(
                "aihubmix/{wire} · {aspect} · {} bytes (gemini-native)",
                bytes.len()
            ),
            bytes,
            ext: ".png",
        });
    }
    let mut body = json!({
        "model": wire,
        "prompt": prompt_or(job, DEFAULT_IMAGE_PROMPT),
        "n": 1,
        "size": openai_size(&wire, job.aspect.as_deref()),
    });
    if wire.starts_with("dall-e-") {
        body["response_format"] = json!("b64_json");
        body["quality"] = json!(if wire == "dall-e-3" { "hd" } else { "standard" });
    } else {
        body["quality"] = json!("high");
    }
    let url = generation_url(&base, "images");
    let res = post(http, &url, Some(k), &[AIHUBMIX_APP_CODE], &body, "aihubmix").await?;
    let data = json_of(res, "aihubmix").await?;
    let entry = data
        .pointer("/data/0")
        .ok_or_else(|| MediaError::Upstream("aihubmix response had no data[0]".to_owned()))?;
    let bytes = if let Some(s) = entry
        .get("b64_json")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        b64(s, "aihubmix")?
    } else if let Some(u) = entry.get("url").and_then(Value::as_str) {
        download_external(u, &base, None, "aihubmix image fetch").await?
    } else {
        return Err(MediaError::Upstream(
            "aihubmix response had neither b64_json nor url".to_owned(),
        ));
    };
    Ok(Rendered {
        note: note(&[
            &format!("aihubmix/{wire}"),
            &aspect,
            &format!("{} bytes", bytes.len()),
        ]),
        bytes,
        ext: ".png",
    })
}

async fn aihubmix_tts(
    http: &reqwest::Client,
    def: ModelDef,
    job: &Job,
    creds: &Credentials,
) -> Result<Rendered, MediaError> {
    let k = key(creds, Provider::AiHubMix)?;
    let wire = aihubmix_wire(def, creds);
    let voice = job
        .voice
        .as_deref()
        .map(str::trim)
        .filter(|v| OPENAI_TTS_VOICES.contains(v))
        .unwrap_or("alloy");
    let body = json!({"model": wire, "input": prompt_or(job, DEFAULT_SPEECH_TEXT), "voice": voice, "response_format": "mp3"});
    let url = speech_url(&base(creds, Provider::AiHubMix));
    let res = post(
        http,
        &url,
        Some(k),
        &[AIHUBMIX_APP_CODE],
        &body,
        "aihubmix speech",
    )
    .await?;
    let bytes = bytes_of(res, "aihubmix speech").await?;
    Ok(Rendered {
        note: note(&[
            &format!("aihubmix/{wire}"),
            voice,
            "mp3",
            &format!("{} bytes", bytes.len()),
        ]),
        bytes,
        ext: ".mp3",
    })
}

/// Seconds AIHubMix's video models accept, snapped to the nearest.
fn aihubmix_video_seconds(model: &str, requested: f64) -> String {
    let m = model.to_ascii_lowercase();
    let allowed: &[f64] = if m.starts_with("veo") {
        &[4.0, 6.0, 8.0]
    } else if m.starts_with("sora") {
        &[4.0, 8.0, 12.0]
    } else if m.starts_with("wan") {
        &[5.0, 10.0]
    } else {
        return format!("{}", requested.round().clamp(3.0, 12.0));
    };
    let best = allowed
        .iter()
        .copied()
        .min_by(|a, b| (a - requested).abs().total_cmp(&(b - requested).abs()))
        .unwrap_or(allowed[0]);
    format!("{best}")
}

async fn aihubmix_video(
    http: &reqwest::Client,
    def: ModelDef,
    job: &Job,
    creds: &Credentials,
    limits: Limits,
) -> Result<Rendered, MediaError> {
    let k = key(creds, Provider::AiHubMix)?;
    let base = base(creds, Provider::AiHubMix);
    let wire = aihubmix_wire(def, creds);
    let size = match job.aspect.as_deref() {
        Some("9:16") => "720x1280",
        Some("1:1") => "1024x1024",
        Some("4:3") => "960x720",
        Some("3:4") => "720x960",
        _ => "1280x720",
    };
    let seconds = aihubmix_video_seconds(&wire, job.length.unwrap_or(5.0));
    let body = json!({"model": wire, "prompt": prompt_or(job, DEFAULT_VIDEO_PROMPT), "size": size, "seconds": seconds});
    let res = post(
        http,
        &format!("{base}/videos"),
        Some(k),
        &[AIHUBMIX_APP_CODE],
        &body,
        "aihubmix video submit",
    )
    .await?;
    let submit = json_of(res, "aihubmix video submit").await?;
    let id = submit
        .get("id")
        .or_else(|| submit.pointer("/data/id"))
        .and_then(Value::as_str)
        .ok_or_else(|| MediaError::Upstream("aihubmix video response missing id".to_owned()))?
        .to_owned();
    let done = poll_until(
        http,
        &format!("{base}/videos/{id}"),
        k,
        &[AIHUBMIX_APP_CODE],
        (limits.poll_interval, Duration::from_secs(12 * 60)),
        "aihubmix video",
        |p| {
            let status = p
                .get("status")
                .or_else(|| p.pointer("/data/status"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            match status {
                "completed" | "succeeded" | "done" => Ok(Some(p.clone())),
                "failed" | "cancelled" | "error" => Err(MediaError::Upstream(format!(
                    "aihubmix video task {status}: {}",
                    truncate(
                        &p.pointer("/error/message")
                            .or_else(|| p.get("error"))
                            .map_or_else(
                                || status.to_owned(),
                                |e| e.as_str().map_or_else(|| e.to_string(), str::to_owned)
                            ),
                        200
                    )
                ))),
                _ => Ok(None),
            }
        },
    )
    .await?;
    let direct = [
        "/video_url",
        "/url",
        "/output_url",
        "/data/video_url",
        "/data/url",
        "/data/0/url",
    ]
    .iter()
    .find_map(|p| done.pointer(p).and_then(Value::as_str))
    .map(str::to_owned);
    let bearer = format!("Bearer {k}");
    let bytes = match direct {
        Some(url) => {
            download_external(
                &url,
                &base,
                Some(("authorization", &bearer)),
                "aihubmix video fetch",
            )
            .await?
        }
        None => {
            let res = http
                .get(format!("{base}/videos/{id}/content"))
                .bearer_auth(k)
                .header(AIHUBMIX_APP_CODE.0, AIHUBMIX_APP_CODE.1)
                .send()
                .await
                .map_err(|e| upstream("aihubmix video content", e))?;
            bytes_of(res, "aihubmix video content").await?
        }
    };
    Ok(Rendered {
        note: note(&[
            &format!("aihubmix/{wire}"),
            size,
            &format!("{seconds}s"),
            &format!("{} bytes", bytes.len()),
        ]),
        bytes,
        ext: ".mp4",
    })
}

/// A 1×1 PNG, a minimal MP4 (`ftyp` + empty `mdat`) or a silent MP3
/// frame (`renderStub`).
#[must_use]
pub fn stub(surface: Surface, model: &str, why: &str) -> Rendered {
    const PNG: [u8; 67] = [
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    const MP4: [u8; 32] = [
        0x00, 0x00, 0x00, 0x18, 0x66, 0x74, 0x79, 0x70, 0x69, 0x73, 0x6f, 0x6d, 0x00, 0x00, 0x02,
        0x00, 0x69, 0x73, 0x6f, 0x6d, 0x69, 0x73, 0x6f, 0x32, 0x00, 0x00, 0x00, 0x08, 0x6d, 0x64,
        0x61, 0x74,
    ];
    const MP3: [u8; 12] = [0xff, 0xfb, 0x90, 0x44, 0, 0, 0, 0, 0, 0, 0, 0];
    let (bytes, ext): (&[u8], &'static str) = match surface {
        Surface::Image => (&PNG, ".png"),
        Surface::Video => (&MP4, ".mp4"),
        Surface::Audio => (&MP3, ".mp3"),
    };
    let kind = match surface {
        Surface::Image => "image",
        Surface::Video => "video",
        Surface::Audio => "audio",
    };
    Rendered {
        bytes: bytes.to_vec(),
        ext,
        note: format!("[stub] stub-{kind} · model={model} · {why}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_compatible_urls_keep_what_was_typed() {
        assert_eq!(
            generation_url("https://x/v1", "images"),
            "https://x/v1/images/generations"
        );
        assert_eq!(
            generation_url("https://x/v1/", "videos"),
            "https://x/v1/videos/generations"
        );
        assert_eq!(
            generation_url("https://x/v1/images/generations", "images"),
            "https://x/v1/images/generations"
        );
        assert_eq!(
            generation_url("https://x/v1/images/edits?k=1", "images"),
            "https://x/v1/images/generations?k=1"
        );
        assert_eq!(speech_url("https://x/v1/"), "https://x/v1/audio/speech");
    }

    #[test]
    fn provider_specific_mappings() {
        assert_eq!(
            seedance_prompt("cat", "9:16", 5.0),
            "cat --resolution 720p --duration 5 --ratio 9:16"
        );
        assert_eq!(
            seedance_prompt("cat --ratio 1:1", "9:16", 8.0),
            "cat --ratio 1:1 --resolution 720p --duration 8"
        );
        assert_eq!(aihubmix_video_seconds("veo-3", 5.0), "4");
        assert_eq!(aihubmix_video_seconds("sora-2", 10.0), "8");
        assert_eq!(aihubmix_video_seconds("kling", 20.0), "12");
        assert!(aihubmix_is_gemini("gemini-2.5-flash-image"));
        assert!(!aihubmix_is_gemini("gemini-2.5-flash-nothink"));
        assert!(!aihubmix_is_gemini("gpt-image-1"));
        assert_eq!(imagerouter_size(Some("16:9"), Surface::Image), "1024x576");
        assert_eq!(imagerouter_size(None, Surface::Image), "1024x1024");
        assert_eq!(imagerouter_size(None, Surface::Video), "1024x576");
        assert_eq!(imagerouter_size(Some("1:1"), Surface::Video), "1024x1024");
        assert!(is_internal("127.0.0.1".parse().unwrap()));
        assert!(is_internal("10.1.2.3".parse().unwrap()));
        assert!(is_internal("::ffff:192.168.1.1".parse().unwrap()));
        assert!(!is_internal("93.184.216.34".parse().unwrap()));
    }

    #[test]
    fn stubs_are_valid_containers() {
        assert_eq!(
            sniff_image_ext(&stub(Surface::Image, "m", "x").bytes),
            ".png"
        );
        assert!(stub(Surface::Video, "m", "x").bytes[4..8] == *b"ftyp");
        assert!(
            stub(Surface::Audio, "m", "x")
                .note
                .starts_with("[stub] stub-audio")
        );
    }
}
