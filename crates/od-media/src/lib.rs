//! Media generation (spec 1.5, 4.2): images, video and audio from OpenAI,
//! fal.ai, ElevenLabs and the providers of [`extra`] with the user's own
//! keys, plus Tavily research ([`research`]). The daemon writes the
//! bytes into the project; this crate only talks to providers and keeps
//! `media-config.json`. Nothing is called without a key the user set.

pub mod config;
pub mod extra;
pub mod provider;
pub mod research;
pub mod tasks;

pub use config::{ConfigFile, Credentials, EnvLookup, KeySource, process_env};
pub use provider::{Job, Limits, MediaError, ModelDef, Provider, Rendered, Surface, find_model};
pub use research::{ResearchError, ResearchFindings, ResearchRequest};

use serde::Deserialize;
use serde_json::{Value, json};

/// `POST /api/projects/:id/media/generate` body (main fields of the
/// original).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateRequest {
    pub surface: Surface,
    pub model: String,
    #[serde(default)]
    pub prompt: String,
    /// Project path; the extension follows what the provider returned.
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub aspect: Option<String>,
    #[serde(default)]
    pub length: Option<f64>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub voice: Option<String>,
}

/// A finished generation, ready to be written at `name`.
#[derive(Debug, Clone, PartialEq)]
pub struct Generated {
    pub name: String,
    pub bytes: Vec<u8>,
    pub model: String,
    pub surface: Surface,
    /// Provider id (`openai`, `custom-image`, or the pending provider a
    /// placeholder stood in for).
    pub provider: &'static str,
    pub note: String,
}

/// `name` with its extension replaced by `ext` (which has the dot).
#[must_use]
pub fn with_extension(name: &str, ext: &str) -> String {
    let (dir, file) = name.rsplit_once('/').map_or(("", name), |(d, f)| (d, f));
    let stem = file
        .rsplit_once('.')
        .filter(|(s, _)| !s.is_empty())
        .map_or(file, |(s, _)| s);
    if dir.is_empty() {
        format!("{stem}{ext}")
    } else {
        format!("{dir}/{stem}{ext}")
    }
}

fn default_name(model: &str, stamp_ms: u128) -> String {
    let slug: String = model
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    format!("media/{}-{stamp_ms}", slug.trim_matches('-'))
}

/// Runs one generation.
///
/// # Errors
/// Unknown model, missing key, provider failure.
pub async fn generate(
    http: &reqwest::Client,
    config: &ConfigFile,
    env: &EnvLookup,
    req: &GenerateRequest,
    limits: Limits,
) -> Result<Generated, MediaError> {
    let model = req.model.trim();
    let job = Job {
        surface: req.surface,
        model: model.to_owned(),
        prompt: req.prompt.trim().to_owned(),
        aspect: req.aspect.clone(),
        length: req.length,
        duration: req.duration,
        voice: req.voice.clone(),
    };
    let (rendered, provider) = match find_model(model, req.surface) {
        Some(def) => render_or_stub(http, config, env, def, &job, limits).await?,
        None => {
            let pending = provider::PENDING_MODELS
                .iter()
                .find(|(id, _, surface)| *id == model && *surface == req.surface);
            match pending {
                Some((_, provider, _)) if limits.allow_stubs => (
                    extra::stub(
                        req.surface,
                        model,
                        &format!("provider '{provider}' integration pending"),
                    ),
                    *provider,
                ),
                Some(_) => {
                    return Err(MediaError::BadRequest(format!(
                        "provider not configured: {model}. Add your API key in Settings → Media to enable real generation."
                    )));
                }
                None => {
                    return Err(MediaError::BadRequest(format!(
                        "unknown media model {:?}",
                        req.model
                    )));
                }
            }
        }
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let base = req
        .output
        .as_deref()
        .map(str::trim)
        .filter(|o| !o.is_empty())
        .map_or_else(|| default_name(&job.model, stamp), str::to_owned);
    Ok(Generated {
        name: with_extension(&base, rendered.ext),
        bytes: rendered.bytes,
        model: job.model,
        surface: req.surface,
        provider,
        note: rendered.note,
    })
}

/// The provider call for `def`, or a placeholder in its place when it
/// failed and [`Limits::allow_stubs`] is on.
async fn render_or_stub(
    http: &reqwest::Client,
    config: &ConfigFile,
    env: &EnvLookup,
    def: ModelDef,
    job: &Job,
    limits: Limits,
) -> Result<(Rendered, &'static str), MediaError> {
    let mut creds = config.credentials(def.provider, env);
    if def.provider == Provider::MiniMax && def.surface == Surface::Image {
        // Image generation is on another MiniMax host than speech.
        creds.base_url = env("OD_MINIMAX_IMAGE_BASE_URL").filter(|u| !u.trim().is_empty());
    }
    // A Custom Image API that names this OpenAI model takes it over.
    let custom = (def.provider == Provider::OpenAi && def.surface == Surface::Image)
        .then(|| config.credentials(Provider::CustomImage, env))
        .filter(|c| extra::custom_image_overrides(c, def));
    let (result, provider) = match custom {
        Some(c) => (
            extra::custom_image(http, def, job, &c).await,
            Provider::CustomImage,
        ),
        None => (
            provider::render(http, def, job, &creds, limits).await,
            def.provider,
        ),
    };
    match result {
        Ok(r) => Ok((r, provider.id())),
        Err(e) if limits.allow_stubs => {
            tracing::warn!(provider = provider.id(), model = def.id, error = %e, "media provider failed; writing a placeholder");
            let mut stub = extra::stub(def.surface, def.id, "placeholder");
            stub.note = format!("[stub] [{} error → stub] {e}", provider.id());
            Ok((stub, provider.id()))
        }
        Err(e) => Err(e),
    }
}

/// `OD_MEDIA_ALLOW_STUBS` is `1` or `true`.
#[must_use]
pub fn stubs_allowed(env: &EnvLookup) -> bool {
    env("OD_MEDIA_ALLOW_STUBS").is_some_and(|v| v == "1" || v == "true")
}

/// `GET /api/media/models`: the catalogue with each provider's state.
#[must_use]
pub fn models_json(config: &ConfigFile, env: &EnvLookup) -> Value {
    let providers: Vec<Value> = Provider::ALL
        .iter()
        .map(|p| {
            json!({
                "id": p.id(),
                "configured": config.credentials(*p, env).api_key.is_some(),
            })
        })
        .collect();
    json!({ "providers": providers, "models": provider::all_models().collect::<Vec<_>>() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_names() {
        assert_eq!(with_extension("hero.jpg", ".png"), "hero.png");
        assert_eq!(with_extension("a/b/clip", ".mp4"), "a/b/clip.mp4");
        assert_eq!(with_extension(".hidden", ".png"), ".hidden.png");
        assert_eq!(
            default_name("fal-ai/flux/dev", 7),
            "media/fal-ai-flux-dev-7"
        );
    }
}
