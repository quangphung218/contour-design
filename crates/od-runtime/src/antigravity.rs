//! Antigravity model choice (ports the settings half of
//! `defs/antigravity.ts`).
//!
//! `agy` has no `--model`: every headless run reads the model its TUI last
//! saved in `~/.gemini/antigravity-cli/settings.json`. A run with a chosen
//! model writes it there just before spawn. The file is shared by every
//! run, so runs that set a model take turns: the next one may write only
//! after this run's agy logged that it read the file
//! (`Propagating selected model override to backend: label="…"` in its
//! `--log-file`), or after this run's process is gone.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::Mutex;

/// How often the agent log is checked for the model line.
const LOG_POLL: Duration = Duration::from_millis(250);

/// `<home>/.gemini/antigravity-cli/settings.json`.
#[must_use]
pub fn settings_path(home: &Path) -> PathBuf {
    home.join(".gemini")
        .join("antigravity-cli")
        .join("settings.json")
}

/// Sets `model` in the settings file, keeping its other keys; a corrupt
/// file is rewritten from scratch.
///
/// # Errors
/// I/O errors creating the folder or writing the file.
pub fn write_model(path: &Path, model: &str) -> std::io::Result<()> {
    let mut obj = std::fs::read(path)
        .ok()
        .and_then(|raw| serde_json::from_slice::<serde_json::Value>(&raw).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    obj.insert("model".to_owned(), serde_json::Value::String(model.to_owned()));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut text = serde_json::to_string_pretty(&serde_json::Value::Object(obj))
        .map_err(std::io::Error::other)?;
    text.push('\n');
    std::fs::write(path, text)
}

/// The log line agy writes once it has read the model from settings.
#[must_use]
pub fn model_read_line(model: &str) -> String {
    format!("Propagating selected model override to backend: label=\"{model}\"")
}

fn lock() -> Arc<Mutex<()>> {
    static LOCK: OnceLock<Arc<Mutex<()>>> = OnceLock::new();
    Arc::clone(LOCK.get_or_init(Arc::default))
}

/// Holds the settings file for one run; dropping it (the attempt ended)
/// lets the next run write.
#[derive(Debug)]
pub struct ModelLock {
    watcher: tokio::task::JoinHandle<()>,
}

impl Drop for ModelLock {
    fn drop(&mut self) {
        self.watcher.abort();
    }
}

/// Waits for the settings file, writes `model` into it and keeps it until
/// `log` shows agy read it. A write failure is logged and the run goes on
/// with whatever model agy has.
pub async fn select_model(settings: PathBuf, model: String, log: PathBuf) -> ModelLock {
    let guard = lock().lock_owned().await;
    if let Err(e) = write_model(&settings, &model) {
        tracing::warn!(error = %e, path = %settings.display(), "writing the Antigravity model");
    }
    let line = model_read_line(&model);
    let watcher = tokio::spawn(async move {
        loop {
            if tokio::fs::read_to_string(&log)
                .await
                .is_ok_and(|text| text.contains(&line))
            {
                break;
            }
            tokio::time::sleep(LOG_POLL).await;
        }
        drop(guard);
    });
    ModelLock { watcher }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_model_is_merged_into_the_settings() {
        let tmp = tempfile::tempdir().unwrap();
        let path = settings_path(tmp.path());
        write_model(&path, "Gemini 3.1 Pro (High)").unwrap();
        std::fs::write(&path, r#"{"theme":"dark","model":"old"}"#).unwrap();
        write_model(&path, "Claude Opus 4.6 (Thinking)").unwrap();
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["model"], "Claude Opus 4.6 (Thinking)");
        std::fs::write(&path, "{not json").unwrap();
        write_model(&path, "X").unwrap();
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(v, serde_json::json!({"model": "X"}));
    }

    /// A second run waits until the first run's agy logged the model.
    #[tokio::test]
    async fn runs_take_turns_until_the_model_is_read() {
        let tmp = tempfile::tempdir().unwrap();
        let settings = settings_path(tmp.path());
        let log_a = tmp.path().join("a.log");
        let first = select_model(settings.clone(), "A".to_owned(), log_a.clone()).await;
        let second = tokio::spawn({
            let settings = settings.clone();
            let log_b = tmp.path().join("b.log");
            async move { select_model(settings, "B".to_owned(), log_b).await }
        });
        tokio::time::sleep(Duration::from_millis(600)).await;
        assert!(!second.is_finished(), "B wrote before A's agy read the file");
        let read = |p: &Path| {
            let v: serde_json::Value = serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap();
            v["model"].as_str().unwrap().to_owned()
        };
        assert_eq!(read(&settings), "A");
        std::fs::write(&log_a, format!("x\n{}\n", model_read_line("A"))).unwrap();
        let b = tokio::time::timeout(Duration::from_secs(5), second)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(read(&settings), "B");
        drop(first);
        // An attempt that ends before the line releases the file too.
        drop(b);
        let c = tokio::time::timeout(
            Duration::from_secs(5),
            select_model(settings.clone(), "C".to_owned(), tmp.path().join("c.log")),
        )
        .await
        .unwrap();
        assert_eq!(read(&settings), "C");
        drop(c);
    }
}
