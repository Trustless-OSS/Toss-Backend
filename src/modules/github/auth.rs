use std::time::{Duration, Instant};

use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use crate::{
    error::AppError,
    infra::cache_keys,
    modules::{
        github::repository::{get_github_repo_id_by_full_name, update_repo_installation_id},
        repo::repository::{get_repo_by_github_id, invalidate_repo_cache},
    },
    state::AppState,
};

static COMMENT_CACHE: Mutex<Option<(String, Instant)>> = Mutex::const_new(None);
const GITHUB_ACCEPT: &str = "application/vnd.github+json";
const GITHUB_API_VERSION: &str = "2022-11-28";
const GITHUB_USER_AGENT: &str = "Trustless-OSS-Bot";

#[derive(Serialize)]
struct AppClaims {
    iat: i64,
    exp: i64,
    /// GitHub requires a numeric App ID. A string `iss` makes
    /// `POST /app/installations/{id}/access_tokens` return 404.
    iss: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct InstallationTokenResponse {
    token: String,
}

#[derive(Debug, Deserialize)]
struct InstallationResponse {
    id: i64,
}

#[derive(Debug, Deserialize)]
struct InstallationReposResponse {
    repositories: Vec<InstallationRepo>,
}

#[derive(Debug, Deserialize)]
pub struct InstallationRepo {
    pub id: i64,
    pub full_name: String,
    pub private: bool,
    pub fork: bool,
    pub owner: InstallationRepoOwner,
}

#[derive(Debug, Deserialize)]
pub struct InstallationRepoOwner {
    pub id: i64,
    pub login: String,
    #[serde(rename = "type")]
    pub account_type: String,
}

fn normalize_private_key(private_key: &str) -> String {
    let mut normalized = private_key.trim().to_string();
    if normalized.starts_with('"') && normalized.ends_with('"') {
        normalized = normalized[1..normalized.len() - 1].to_string();
    }

    if normalized.contains("-----BEGIN") {
        normalized.replace("\\n", "\n")
    } else {
        let b64_body: String = normalized
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect();
        let wrapped = b64_body
            .as_bytes()
            .chunks(64)
            .map(|chunk| std::str::from_utf8(chunk).unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
        format!("-----BEGIN RSA PRIVATE KEY-----\n{wrapped}\n-----END RSA PRIVATE KEY-----")
    }
}

fn github_app_issuer(app_id: &str) -> serde_json::Value {
    let trimmed = app_id.trim();
    trimmed
        .parse::<u64>()
        .map(serde_json::Value::from)
        .unwrap_or_else(|_| serde_json::Value::String(trimmed.to_string()))
}

async fn create_app_jwt(state: &AppState) -> Result<String, AppError> {
    let app_id = state.config.github_app_id.as_str();
    let private_key = state.config.github_app_private_key.as_str();

    let now = chrono::Utc::now().timestamp();
    let claims = AppClaims {
        iat: now - 30,
        exp: now + 540,
        iss: github_app_issuer(app_id),
    };

    encode(
        &Header::new(Algorithm::RS256),
        &claims,
        &EncodingKey::from_rsa_pem(normalize_private_key(private_key).as_bytes()).map_err(
            |error| AppError::github(format!("invalid GitHub app private key: {error}")),
        )?,
    )
    .map_err(|error| AppError::github(format!("failed to create GitHub app JWT: {error}")))
}

pub async fn get_installation_token(
    state: &AppState,
    github_repo_id: i64,
) -> Result<Option<String>, AppError> {
    let cache_key = cache_keys::gh_token(github_repo_id);
    if let Some(cached) = state.cache.get::<String>(&cache_key).await {
        return Ok(Some(cached));
    }

    let repo = get_repo_by_github_id(state, github_repo_id)
        .await?
        .ok_or_else(|| AppError::github("repository not found for installation token"))?;

    if let Some(installation_id) = repo.github_installation_id {
        match exchange_installation_token(state, installation_id).await {
            Ok(token) => {
                cache_installation_token(state, &cache_key, &token).await;
                return Ok(Some(token));
            }
            Err(error) => {
                warn!(
                    %error,
                    repo = %repo.full_name,
                    installation_id,
                    "stored GitHub installation ID failed; attempting auto-repair"
                );
            }
        }
    }

    let installation_id = resolve_installation_id(state, &repo.full_name).await?;
    if repo.github_installation_id != Some(installation_id) {
        update_repo_installation_id(state, github_repo_id, installation_id).await?;
        invalidate_repo_cache(state, repo.id, Some(github_repo_id)).await;
        info!(
            repo = %repo.full_name,
            installation_id,
            "GitHub installation ID repaired"
        );
    }

    let token = exchange_installation_token(state, installation_id).await?;
    cache_installation_token(state, &cache_key, &token).await;
    Ok(Some(token))
}

async fn resolve_installation_id(state: &AppState, full_name: &str) -> Result<i64, AppError> {
    let (owner, name) = full_name
        .split_once('/')
        .ok_or_else(|| AppError::github("invalid repository full_name"))?;
    let jwt = create_app_jwt(state).await?;
    let url = format!("https://api.github.com/repos/{owner}/{name}/installation");
    let response = state
        .http_client
        .get(&url)
        .header("Authorization", format!("Bearer {jwt}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "Trustless-OSS-Bot")
        .send()
        .await
        .map_err(|error| AppError::github(format!("installation lookup failed: {error}")))?;

    github_json::<InstallationResponse>(response, "installation lookup")
        .await
        .map(|installation| installation.id)
}

async fn exchange_installation_token(
    state: &AppState,
    installation_id: i64,
) -> Result<String, AppError> {
    let jwt = create_app_jwt(state).await?;
    let url = format!("https://api.github.com/app/installations/{installation_id}/access_tokens");
    let response = state
        .http_client
        .post(&url)
        .header("Authorization", format!("Bearer {jwt}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "Trustless-OSS-Bot")
        .send()
        .await
        .map_err(|error| AppError::github(format!("token exchange failed: {error}")))?;

    github_json::<InstallationTokenResponse>(response, "installation token exchange")
        .await
        .map(|payload| payload.token)
}

async fn github_json<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
    operation: &str,
) -> Result<T, AppError> {
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|error| AppError::github(format!("{operation} response failed: {error}")))?;
    if !status.is_success() {
        let message = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|payload| {
                payload
                    .get("message")
                    .and_then(|value| value.as_str())
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| text.chars().take(300).collect());
        let hint = if status.as_u16() == 404 && operation.contains("installation") {
            " GitHub App ID/private key likely do not match the installed app, or the JWT iss claim is not a numeric App ID."
        } else {
            ""
        };
        return Err(AppError::github(format!(
            "{operation} failed with {status}: {message}.{hint}"
        )));
    }

    serde_json::from_str(&text)
        .map_err(|error| AppError::github(format!("invalid {operation} response: {error}")))
}

async fn cache_installation_token(state: &AppState, cache_key: &str, token: &str) {
    state
        .cache
        .set(
            cache_key,
            &token.to_string(),
            Some(cache_keys::GH_TOKEN_TTL),
        )
        .await;
}

pub async fn list_installation_repos(
    state: &AppState,
    installation_id: i64,
) -> Result<Vec<InstallationRepo>, AppError> {
    let token = exchange_installation_token(state, installation_id).await?;
    let response = state
        .http_client
        .get("https://api.github.com/installation/repositories?per_page=100")
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", GITHUB_ACCEPT)
        .header("X-GitHub-Api-Version", GITHUB_API_VERSION)
        .header("User-Agent", GITHUB_USER_AGENT)
        .send()
        .await
        .map_err(|error| AppError::github(error.to_string()))?;

    github_json::<InstallationReposResponse>(response, "installation repository listing")
        .await
        .map(|payload| payload.repositories)
}

pub async fn post_comment(
    state: &AppState,
    full_name: &str,
    issue_number: i32,
    body: &str,
) -> Result<(), AppError> {
    let cache_key = format!(
        "{}#{}:{}",
        full_name,
        issue_number,
        &body[..body.len().min(50)]
    );
    {
        let guard = COMMENT_CACHE.lock().await;
        if let Some((key, sent_at)) = guard.as_ref() {
            if key == &cache_key && sent_at.elapsed() < Duration::from_secs(10) {
                debug!(
                    repo = full_name,
                    issue = issue_number,
                    "skipping duplicate comment"
                );
                return Ok(());
            }
        }
    }

    let github_repo_id = get_github_repo_id_by_full_name(state, full_name).await?;

    let Some(github_repo_id) = github_repo_id else {
        error!(
            repo = full_name,
            "cannot post comment — repository not found"
        );
        return Ok(());
    };

    let Some(token) = get_installation_token(state, github_repo_id).await? else {
        error!(
            repo = full_name,
            "cannot post comment — failed to get token"
        );
        return Ok(());
    };

    let url = format!("https://api.github.com/repos/{full_name}/issues/{issue_number}/comments");
    let response = state
        .http_client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", GITHUB_ACCEPT)
        .header("X-GitHub-Api-Version", GITHUB_API_VERSION)
        .header("User-Agent", GITHUB_USER_AGENT)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({ "body": body }))
        .send()
        .await
        .map_err(|error| AppError::github(error.to_string()))?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        let message = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|payload| {
                payload
                    .get("message")
                    .and_then(|value| value.as_str())
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| text.chars().take(300).collect());
        return Err(AppError::github(format!(
            "failed to post comment to {full_name}#{issue_number} with {status}: {message}"
        )));
    }

    let mut guard = COMMENT_CACHE.lock().await;
    *guard = Some((cache_key, Instant::now()));

    Ok(())
}

pub async fn install_repo_webhook(
    state: &AppState,
    full_name: &str,
    gh_token: &str,
    webhook_url: &str,
) -> Result<(), AppError> {
    let secret = state.config.github_webhook_secret.as_str();

    let hooks_url = format!("https://api.github.com/repos/{full_name}/hooks");
    let existing = state
        .http_client
        .get(&hooks_url)
        .header("Authorization", format!("Bearer {gh_token}"))
        .header("Accept", GITHUB_ACCEPT)
        .header("X-GitHub-Api-Version", GITHUB_API_VERSION)
        .header("User-Agent", GITHUB_USER_AGENT)
        .send()
        .await
        .map_err(|error| AppError::github(error.to_string()))?;

    let mut already_exists = false;
    if existing.status().is_success() {
        let hooks: Vec<serde_json::Value> = existing
            .json()
            .await
            .map_err(|error| AppError::github(error.to_string()))?;
        already_exists = hooks.iter().any(|hook| {
            hook.get("config")
                .and_then(|config| config.get("url"))
                .and_then(|url| url.as_str())
                == Some(webhook_url)
        });
    }

    if already_exists {
        return Ok(());
    }

    let response = state
        .http_client
        .post(&hooks_url)
        .header("Authorization", format!("Bearer {gh_token}"))
        .header("Accept", GITHUB_ACCEPT)
        .header("X-GitHub-Api-Version", GITHUB_API_VERSION)
        .header("User-Agent", GITHUB_USER_AGENT)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "name": "web",
            "active": true,
            "events": ["issues", "pull_request", "issue_comment"],
            "config": {
                "url": webhook_url,
                "content_type": "json",
                "secret": secret,
                "insecure_ssl": "0"
            }
        }))
        .send()
        .await
        .map_err(|error| AppError::github(error.to_string()))?;

    if !response.status().is_success() {
        error!(repo = full_name, status = %response.status(), "failed to install webhook");
    }

    Ok(())
}

pub async fn fetch_github_issue_state(
    state: &AppState,
    full_name: &str,
    issue_number: i32,
) -> Result<String, AppError> {
    let token = state
        .config
        .github_bot_token
        .as_deref()
        .ok_or_else(|| AppError::github("GITHUB_BOT_TOKEN is not configured"))?;

    let url = format!("https://api.github.com/repos/{full_name}/issues/{issue_number}");
    let response = state
        .http_client
        .get(&url)
        .header("Authorization", format!("token {token}"))
        .header("Accept", "application/vnd.github.v3+json")
        .header("User-Agent", "Trustless-OSS-Bot")
        .send()
        .await
        .map_err(|error| AppError::github(error.to_string()))?;

    let payload: serde_json::Value = response
        .json()
        .await
        .map_err(|error| AppError::github(error.to_string()))?;

    payload
        .get("state")
        .and_then(|value| value.as_str())
        .map(str::to_owned)
        .ok_or_else(|| AppError::github("GitHub issue state missing from response"))
}

pub async fn delete_github_installation(
    state: &AppState,
    installation_id: i64,
) -> Result<(), AppError> {
    let jwt = create_app_jwt(state).await?;
    let url = format!("https://api.github.com/app/installations/{installation_id}");
    state
        .http_client
        .delete(&url)
        .header("Authorization", format!("Bearer {jwt}"))
        .header("Accept", "application/vnd.github.v3+json")
        .header("User-Agent", "Trustless-OSS-Bot")
        .send()
        .await
        .map_err(|error| AppError::github(error.to_string()))?;
    Ok(())
}

pub async fn remove_repo_from_installation(
    state: &AppState,
    installation_id: i64,
    repository_id: i64,
) -> Result<(), AppError> {
    let jwt = create_app_jwt(state).await?;
    let url = format!(
        "https://api.github.com/app/installations/{installation_id}/repositories/{repository_id}"
    );
    state
        .http_client
        .request(Method::DELETE, &url)
        .header("Authorization", format!("Bearer {jwt}"))
        .header("Accept", "application/vnd.github.v3+json")
        .header("User-Agent", "Trustless-OSS-Bot")
        .send()
        .await
        .map_err(|error| AppError::github(error.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{github_app_issuer, normalize_private_key, AppClaims};

    #[test]
    fn github_app_jwt_iss_is_numeric_for_app_id() {
        let claims = AppClaims {
            iat: 1,
            exp: 2,
            iss: github_app_issuer(" 153860735 "),
        };
        let json = serde_json::to_value(&claims).expect("claims should serialize");
        assert!(
            json["iss"].is_number(),
            "GitHub rejects a string iss App ID"
        );
        assert_eq!(json["iss"], 153860735);
    }

    #[test]
    fn github_app_jwt_iss_keeps_client_id_as_string() {
        let claims = AppClaims {
            iat: 1,
            exp: 2,
            iss: github_app_issuer("Iv1.abcd1234"),
        };
        let json = serde_json::to_value(&claims).expect("claims should serialize");
        assert_eq!(json["iss"], "Iv1.abcd1234");
    }

    #[test]
    fn normalize_private_key_unescapes_newlines() {
        let pem = "-----BEGIN PRIVATE KEY-----\\nABC\\n-----END PRIVATE KEY-----";
        let normalized = normalize_private_key(pem);
        assert!(normalized.contains('\n'));
        assert!(!normalized.contains("\\n"));
    }
}
