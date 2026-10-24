//! Agent-callable web research through Tavily with the user's key (ports
//! `research/index.ts` and `research/tavily.ts`): `POST <base>/search`,
//! shallow depth, a bounded number of sources.

use crate::config::{ConfigFile, EnvLookup};
use crate::provider::{Provider, truncate};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

/// Sources returned when the caller does not say.
pub const DEFAULT_MAX_SOURCES: u64 = 5;
/// Tavily's own ceiling.
pub const MAX_SOURCES: u64 = 20;
const TIMEOUT: Duration = Duration::from_secs(30);

/// `POST /api/research/search` body.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchRequest {
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub max_sources: Option<f64>,
    #[serde(default)]
    pub providers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchSource {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub provider: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchFindings {
    pub query: String,
    pub summary: String,
    pub sources: Vec<ResearchSource>,
    pub provider: &'static str,
    pub depth: &'static str,
    pub fetched_at: u128,
}

/// Why a search failed: HTTP status and the original's error code.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ResearchError {
    pub status: u16,
    pub code: &'static str,
    pub message: String,
}

fn fail(status: u16, code: &'static str, message: impl Into<String>) -> ResearchError {
    ResearchError {
        status,
        code,
        message: message.into(),
    }
}

/// Clamps a requested source count to `1..=MAX_SOURCES`.
#[must_use]
pub fn clamp_max_sources(value: Option<f64>) -> u64 {
    match value {
        Some(v) if v.is_finite() && v > 0.0 => {
            // In range after the clamp, so the cast is exact.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let n = v.floor().clamp(1.0, 20.0) as u64;
            n
        }
        _ => DEFAULT_MAX_SOURCES,
    }
}

/// Runs one shallow Tavily search.
///
/// # Errors
/// [`ResearchError`] with the original's codes: `QUERY_REQUIRED`,
/// `UNSUPPORTED_RESEARCH_PROVIDER`, `TAVILY_API_KEY_MISSING`,
/// `RESEARCH_PROVIDER_FAILED`, `NO_RESEARCH_SOURCES`.
pub async fn search(
    http: &reqwest::Client,
    config: &ConfigFile,
    env: &EnvLookup,
    req: &ResearchRequest,
) -> Result<ResearchFindings, ResearchError> {
    let query: String = req.query.trim().chars().take(1000).collect();
    if query.is_empty() {
        return Err(fail(400, "QUERY_REQUIRED", "query required"));
    }
    let provider = req
        .providers
        .iter()
        .find(|p| !p.is_empty())
        .map_or("tavily", String::as_str);
    if provider != "tavily" {
        return Err(fail(
            400,
            "UNSUPPORTED_RESEARCH_PROVIDER",
            format!("provider \"{provider}\" not supported"),
        ));
    }
    let creds = config.credentials(Provider::Tavily, env);
    let Some(key) = creds.api_key else {
        return Err(fail(
            400,
            "TAVILY_API_KEY_MISSING",
            "Tavily API key not configured (Settings → Media → Tavily Search)",
        ));
    };
    let max = clamp_max_sources(req.max_sources);
    let base = creds
        .base_url
        .as_deref()
        .unwrap_or(Provider::Tavily.default_base_url())
        .trim_end_matches('/')
        .to_owned();
    let body = json!({
        "query": query,
        "search_depth": "basic",
        "max_results": max,
        "include_answer": true,
        "include_raw_content": false,
    });
    let provider_failed = |m: String| fail(502, "RESEARCH_PROVIDER_FAILED", m);
    let res = http
        .post(format!("{base}/search"))
        .bearer_auth(&key)
        .json(&body)
        .timeout(TIMEOUT)
        .send()
        .await
        .map_err(|e| provider_failed(format!("Tavily request failed: {e}")))?;
    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    if !status.is_success() {
        let detail = if text.is_empty() {
            "no body".to_owned()
        } else {
            truncate(&text, 200)
        };
        return Err(provider_failed(format!(
            "Tavily {}: {detail}",
            status.as_u16()
        )));
    }
    let data: Value = serde_json::from_str(&text).map_err(|_| {
        provider_failed(format!(
            "Tavily non-JSON response: {}",
            truncate(&text, 200)
        ))
    })?;
    let sources: Vec<ResearchSource> = data
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let url = r
                .get("url")
                .and_then(Value::as_str)
                .filter(|u| !u.is_empty())?;
            let title = r
                .get("title")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .unwrap_or(url);
            Some(ResearchSource {
                title: title.to_owned(),
                url: url.to_owned(),
                snippet: r
                    .get("content")
                    .and_then(Value::as_str)
                    .map(|c| c.trim().chars().take(800).collect())
                    .unwrap_or_default(),
                provider: "tavily",
                published_at: r
                    .get("published_date")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|d| !d.is_empty())
                    .map(str::to_owned),
            })
        })
        .collect();
    if sources.is_empty() {
        return Err(fail(404, "NO_RESEARCH_SOURCES", "no sources found"));
    }
    let answer = data
        .get("answer")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let summary = if answer.is_empty() {
        let lead: Vec<String> = sources
            .iter()
            .take(5)
            .enumerate()
            .map(|(i, s)| {
                let snippet: String = s.snippet.chars().take(200).collect();
                format!("- [{}] {}: {snippet}", i + 1, s.title)
            })
            .collect();
        format!(
            "(No provider summary; top snippets follow.)\n{}",
            lead.join("\n")
        )
    } else {
        answer.to_owned()
    };
    Ok(ResearchFindings {
        query,
        summary,
        sources,
        provider: "tavily",
        depth: "shallow",
        fetched_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_counts_are_clamped() {
        assert_eq!(clamp_max_sources(None), 5);
        assert_eq!(clamp_max_sources(Some(0.0)), 5);
        assert_eq!(clamp_max_sources(Some(f64::NAN)), 5);
        assert_eq!(clamp_max_sources(Some(3.7)), 3);
        assert_eq!(clamp_max_sources(Some(99.0)), MAX_SOURCES);
    }
}
