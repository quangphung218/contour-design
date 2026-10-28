//! Help ▸ Check for Updates and Settings ▸ About: `POST /api/updates/check`
//! asks GitHub Releases for the newest release (pre-releases count, drafts
//! do not) and compares it with this build by semver. Only that request
//! calls out; nothing checks on its own (the spec allows no silent auto-update).

use crate::error::{ApiErr, ApiResult};
use axum::Json;
use axum::http::StatusCode;
use od_contracts::ErrorCode;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// The project's releases, newest first.
pub const RELEASES_URL: &str =
    "https://api.github.com/repos/quangphung218/contour-design/releases?per_page=30";
/// Overrides [`RELEASES_URL`] (the e2e points it at a loopback stub).
pub const RELEASES_URL_ENV: &str = "OD_RELEASES_URL";
const TIMEOUT: Duration = Duration::from_secs(10);

/// What a check found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheck {
    /// This build's version.
    pub current: String,
    /// The newest release, when it is newer than `current`.
    pub update: Option<Release>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub version: String,
    /// The release page: notes and downloads.
    pub url: String,
}

/// The fields used of a GitHub release.
#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
}

/// The highest version among the published releases; tags that are not
/// `v<semver>` or `<semver>` are skipped.
fn newest(releases: Vec<GhRelease>) -> Option<(Version, String)> {
    releases
        .into_iter()
        .filter(|r| !r.draft)
        .filter_map(|r| {
            let tag = r.tag_name.strip_prefix('v').unwrap_or(&r.tag_name);
            Some((Version::parse(tag).ok()?, r.html_url))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
}

/// Why a check failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CheckError {
    /// GitHub answered 403 or 429: its limit for unauthenticated requests
    /// (60 an hour per IP address) is used up.
    #[error("GitHub Releases is rate limiting this address")]
    RateLimited,
    #[error("{0}")]
    Failed(String),
}

/// Fetches `url` (a GitHub releases list) and compares with `current`.
///
/// # Errors
/// [`CheckError::RateLimited`] on 403 or 429; otherwise the request
/// failing, another non-2xx answer, or a body that is not a list.
pub async fn check(url: &str, current: &str) -> Result<UpdateCheck, CheckError> {
    let failed = |e: &dyn std::fmt::Display| CheckError::Failed(e.to_string());
    let now = Version::parse(current).map_err(|e| failed(&format!("this build's version: {e}")))?;
    let http = reqwest::Client::builder()
        .user_agent(concat!("contour/", env!("CARGO_PKG_VERSION")))
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| failed(&e))?;
    let res = http
        .get(url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| failed(&e))?;
    if matches!(
        res.status(),
        reqwest::StatusCode::FORBIDDEN | reqwest::StatusCode::TOO_MANY_REQUESTS
    ) {
        return Err(CheckError::RateLimited);
    }
    let releases: Vec<GhRelease> = res
        .error_for_status()
        .map_err(|e| failed(&e))?
        .json()
        .await
        .map_err(|e| failed(&e))?;
    let update = newest(releases)
        .filter(|(v, _)| *v > now)
        .map(|(v, url)| Release {
            version: v.to_string(),
            url,
        });
    Ok(UpdateCheck {
        current: current.to_owned(),
        update,
    })
}

/// 429 when GitHub rate limits (the UI says to wait), else 502.
pub async fn check_route() -> ApiResult<Json<UpdateCheck>> {
    let url = std::env::var(RELEASES_URL_ENV)
        .ok()
        .filter(|u| !u.trim().is_empty())
        .unwrap_or_else(|| RELEASES_URL.to_owned());
    check(&url, env!("CARGO_PKG_VERSION"))
        .await
        .map(Json)
        .map_err(|e| {
            tracing::warn!(error = %e, "update check failed");
            let status = match e {
                CheckError::RateLimited => StatusCode::TOO_MANY_REQUESTS,
                CheckError::Failed(_) => StatusCode::BAD_GATEWAY,
            };
            ApiErr::new(
                status,
                ErrorCode::UpstreamUnavailable,
                format!("cannot reach GitHub Releases: {e}"),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn release(tag: &str, draft: bool) -> Value {
        json!({"tag_name": tag, "html_url": format!("https://r/{tag}"), "draft": draft, "prerelease": true})
    }

    /// A releases endpoint on loopback answering `status` with `body`.
    async fn stub(status: u16, body: Value) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/releases", listener.local_addr().unwrap());
        let app = axum::Router::new().route(
            "/releases",
            axum::routing::get(move || async move {
                (StatusCode::from_u16(status).unwrap(), Json(body))
            }),
        );
        tokio::spawn(async move { axum::serve(listener, app).await });
        url
    }

    #[tokio::test]
    async fn a_newer_pre_release_is_an_update() {
        let url = stub(
            200,
            json!([
                release("v0.2.0-beta.10", false),
                release("v0.2.0-beta.6", false),
                release("v0.1.0", false)
            ]),
        )
        .await;
        let found = check(&url, "0.2.0-beta.5").await.unwrap();
        assert_eq!(
            found.update,
            Some(Release {
                version: "0.2.0-beta.10".to_owned(),
                url: "https://r/v0.2.0-beta.10".to_owned()
            })
        );
    }

    #[tokio::test]
    async fn the_same_or_an_older_version_is_up_to_date() {
        let url = stub(
            200,
            json!([
                release("v0.2.0-beta.5", false),
                release("v0.2.0-beta.4", false)
            ]),
        )
        .await;
        assert_eq!(check(&url, "0.2.0-beta.5").await.unwrap().update, None);
        assert_eq!(check(&url, "0.2.0").await.unwrap().update, None);
    }

    #[tokio::test]
    async fn drafts_and_other_tags_do_not_count() {
        let url = stub(
            200,
            json!([
                release("v0.3.0", true),
                release("nightly", false),
                release("v0.2.0-beta.5", false)
            ]),
        )
        .await;
        assert_eq!(check(&url, "0.2.0-beta.5").await.unwrap().update, None);
    }

    #[tokio::test]
    async fn a_403_or_429_is_a_rate_limit() {
        for status in [403, 429] {
            let url = stub(status, json!({"message": "API rate limit exceeded"})).await;
            assert_eq!(
                check(&url, "0.2.0-beta.5").await,
                Err(CheckError::RateLimited)
            );
        }
    }

    #[tokio::test]
    async fn an_error_answer_or_no_server_is_an_error() {
        let url = stub(503, json!({"message": "unavailable"})).await;
        assert!(matches!(
            check(&url, "0.2.0-beta.5").await,
            Err(CheckError::Failed(_))
        ));
        let dead = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            format!("http://{}/releases", l.local_addr().unwrap())
        };
        assert!(matches!(
            check(&dead, "0.2.0-beta.5").await,
            Err(CheckError::Failed(_))
        ));
    }
}
