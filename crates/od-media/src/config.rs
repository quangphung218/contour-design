//! `<data>/media-config.json` (spec 3.1): per-provider API key, base URL
//! and model override, set from Settings through `PUT /api/media/config`.
//! Environment variables win over stored keys. Reads mask keys: only the
//! last four characters of a stored key are ever echoed.

use crate::provider::Provider;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const CONFIG_FILE: &str = "media-config.json";

/// One provider's stored settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderEntry {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub api_key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// The file: `{ "providers": { "openai": {…}, … } }`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredConfig {
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderEntry>,
}

/// Where a key came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum KeySource {
    Env,
    Stored,
    Unset,
}

/// What a provider call uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub source: KeySource,
}

/// Looks up an environment variable (tests pass a fixed map).
pub type EnvLookup = dyn Fn(&str) -> Option<String> + Send + Sync;

/// The process environment.
#[must_use]
pub fn process_env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{0}")]
    Invalid(String),
    #[error("media-config.json: {0}")]
    Io(#[from] std::io::Error),
}

/// `media-config.json` under a data folder.
#[derive(Debug, Clone)]
pub struct ConfigFile {
    path: PathBuf,
}

impl ConfigFile {
    #[must_use]
    pub fn in_dir(data_root: &Path) -> Self {
        Self {
            path: data_root.join(CONFIG_FILE),
        }
    }

    /// The stored config; a missing or unreadable file is empty.
    #[must_use]
    pub fn read(&self) -> StoredConfig {
        std::fs::read(&self.path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// Credentials for `provider`: env key first, then the stored key.
    #[must_use]
    pub fn credentials(&self, provider: Provider, env: &EnvLookup) -> Credentials {
        let stored = self
            .read()
            .providers
            .remove(provider.id())
            .unwrap_or_default();
        let env_key = provider
            .env_keys()
            .iter()
            .find_map(|k| env(k).filter(|v| !v.trim().is_empty()));
        let (api_key, source) = match env_key {
            Some(k) => (Some(k.trim().to_owned()), KeySource::Env),
            None if !stored.api_key.is_empty() => (Some(stored.api_key), KeySource::Stored),
            None => (None, KeySource::Unset),
        };
        Credentials {
            api_key,
            base_url: Some(stored.base_url).filter(|u| !u.is_empty()),
            model: stored.model.filter(|m| !m.is_empty()),
            source,
        }
    }

    /// `GET /api/media/config`: every provider, keys masked.
    #[must_use]
    pub fn masked(&self, env: &EnvLookup) -> Value {
        let stored = self.read();
        let mut providers = Map::new();
        for p in Provider::ALL {
            let entry = stored.providers.get(p.id()).cloned().unwrap_or_default();
            let creds = self.credentials(p, env);
            let tail: String = if creds.source == KeySource::Stored {
                let chars: Vec<char> = entry.api_key.chars().collect();
                chars[chars.len().saturating_sub(4)..].iter().collect()
            } else {
                String::new()
            };
            let mut v = json!({
                "configured": creds.api_key.is_some(),
                "source": creds.source,
                "apiKeyTail": tail,
                "baseUrl": entry.base_url,
            });
            if let Some(m) = entry.model {
                v["model"] = json!(m);
            }
            providers.insert(p.id().to_owned(), v);
        }
        json!({ "providers": providers })
    }

    /// `PUT /api/media/config`: `{providers: {id: {apiKey, preserveApiKey,
    /// baseUrl, model}}}` replaces the stored map. An empty `apiKey` with
    /// `preserveApiKey: true` keeps the stored key (the UI only has the
    /// masked tail). Written with owner-only permissions.
    ///
    /// # Errors
    /// Unknown providers, non-http(s) base URLs, I/O.
    pub fn write(&self, body: &Value) -> Result<(), ConfigError> {
        // Read-modify-write of one file: concurrent PUTs take turns.
        static WRITE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = WRITE_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let incoming = body
            .get("providers")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let prior = self.read();
        let mut next = StoredConfig::default();
        for (id, entry) in incoming {
            let provider = Provider::parse(&id)
                .ok_or_else(|| ConfigError::Invalid(format!("unknown media provider {id:?}")))?;
            let text = |k: &str| {
                entry
                    .get(k)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .unwrap_or_default()
                    .to_owned()
            };
            let mut api_key = text("apiKey");
            if api_key.is_empty() && entry.get("preserveApiKey") == Some(&Value::Bool(true)) {
                api_key = prior
                    .providers
                    .get(provider.id())
                    .map(|p| p.api_key.clone())
                    .unwrap_or_default();
            }
            let base_url = text("baseUrl");
            if !(base_url.is_empty()
                || base_url.starts_with("https://")
                || base_url.starts_with("http://"))
            {
                return Err(ConfigError::Invalid(format!(
                    "{id}.baseUrl must be an http(s) URL"
                )));
            }
            let model = Some(text("model")).filter(|m| !m.is_empty());
            if api_key.is_empty() && base_url.is_empty() && model.is_none() {
                continue;
            }
            next.providers.insert(
                provider.id().to_owned(),
                ProviderEntry {
                    api_key,
                    base_url,
                    model,
                },
            );
        }
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut bytes = serde_json::to_vec_pretty(&next).map_err(|e| ConfigError::Io(e.into()))?;
        bytes.push(b'\n');
        write_private(&self.path, &bytes)?;
        Ok(())
    }
}

/// Atomic replace through a fresh, uniquely named temp file next to `path`
/// (`tempfile` creates it `0600`), so no leftover `*.tmp` of another mode
/// or another writer is ever reused.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::Builder::new()
        .prefix(".media-config-")
        .suffix(".tmp")
        .tempfile_in(dir)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[cfg(unix)]
    #[test]
    fn a_leftover_world_readable_temp_file_does_not_leak_its_mode() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let file = ConfigFile::in_dir(tmp.path());
        let stale = tmp.path().join(CONFIG_FILE).with_extension("json.tmp");
        std::fs::write(&stale, "{}").unwrap();
        std::fs::set_permissions(&stale, std::fs::Permissions::from_mode(0o644)).unwrap();
        file.write(&json!({ "providers": { "openai": { "apiKey": "sk-secret-1234" } } }))
            .unwrap();
        let mode = std::fs::metadata(tmp.path().join(CONFIG_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn concurrent_writes_never_fail_or_mix() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let workers: Vec<_> = (0..6)
            .map(|w| {
                let dir = dir.clone();
                std::thread::spawn(move || {
                    let file = ConfigFile::in_dir(&dir);
                    for i in 0..40 {
                        let key = format!("sk-{w}-{i}-{}", "x".repeat(200 * w));
                        file.write(&json!({ "providers": { "openai": { "apiKey": key } } }))
                            .unwrap();
                    }
                })
            })
            .collect();
        for w in workers {
            w.join().unwrap();
        }
        let text = std::fs::read_to_string(dir.join(CONFIG_FILE)).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        let key = v["providers"]["openai"]["apiKey"].as_str().unwrap();
        assert!(key.starts_with("sk-") && key.contains("-39"), "{key}");
    }

    #[test]
    fn keys_are_masked_preserved_and_overridden_by_env() {
        let tmp = tempfile::tempdir().unwrap();
        let file = ConfigFile::in_dir(tmp.path());
        file.write(&json!({ "providers": {
            "openai": { "apiKey": "sk-secret-1234", "baseUrl": "http://127.0.0.1:9/v1" },
            "fal": { "apiKey": "" }
        } }))
        .unwrap();
        let masked = file.masked(&no_env);
        assert_eq!(masked["providers"]["openai"]["apiKeyTail"], "1234");
        assert_eq!(masked["providers"]["openai"]["source"], "stored");
        assert_eq!(masked["providers"]["fal"]["configured"], false);
        assert!(!masked.to_string().contains("sk-secret"));

        // The UI sends back the masked form: the key survives.
        file.write(&json!({ "providers": { "openai": { "apiKey": "", "preserveApiKey": true, "model": "gpt-image-1" } } }))
            .unwrap();
        let creds = file.credentials(Provider::OpenAi, &no_env);
        assert_eq!(creds.api_key.as_deref(), Some("sk-secret-1234"));
        assert_eq!(creds.base_url, None);
        assert_eq!(creds.model.as_deref(), Some("gpt-image-1"));

        let env = |k: &str| (k == "OPENAI_API_KEY").then(|| "sk-env".to_owned());
        let creds = file.credentials(Provider::OpenAi, &env);
        assert_eq!(
            (creds.api_key.as_deref(), creds.source),
            (Some("sk-env"), KeySource::Env)
        );
        assert_eq!(file.masked(&env)["providers"]["openai"]["apiKeyTail"], "");

        assert!(file.write(&json!({ "providers": { "nope": {} } })).is_err());
        assert!(
            file.write(&json!({ "providers": { "fal": { "baseUrl": "file:///etc" } } }))
                .is_err()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(tmp.path().join(CONFIG_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }
}
