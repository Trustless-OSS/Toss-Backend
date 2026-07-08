use std::time::{Duration, Instant};

use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{debug, error, warn};

use crate::{
    error::AppError,
    infra::cache_keys,
    modules::repo::repository::{get_repo_by_github_id, invalidate_repo_cache},
    state::AppState,
};

static COMMENT_CACHE: Mutex<Option<(String, Instant)>> = Mutex::const_new(None);

#[derive(Serialize)]
struct AppClaims {
    iat: i64,
    exp: i64,
    iss: String,
}

#[derive(Debug, Deserialize)]
struct InstallationTokenResponse {
    token: String,
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

async fn create_app_jwt(state: &AppState) -> Result<String, AppError> {
    let app_id = state.config.github_app_id.as_str();
    let private_key = state.config.github_app_private_key.as_str();

    let now = chrono::Utc::now().timestamp();
    let claims = AppClaims {
        iat: now - 60,
        exp: now + 600,
        iss: app_id.to_string(),
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

    let mut installation_id = repo.github_installation_id;

    if installation_id.is_none() {
        let (owner, name) = repo
            .full_name
            .split_once('/')
            .ok_or_else(|| AppError::github("invalid repository full_name"))?;

        let jwt = create_app_jwt(state).await?;
        let url = format!("https://api.github.com/repos/{owner}/{name}/installation");
        let response = state
            .http_client
            .get(&url)
            .header("Authorization", format!("Bearer {jwt}"))
            .header("Accept", "application/vnd.github.v3+json")
            .header("User-Agent", "Trustless-OSS-Bot")
            .send()
            .await
            .map_err(|error| AppError::github(error.to_string()))?;

        if !response.status().is_success() {
            warn!(repo = %repo.full_name, "failed to auto-repair installation id");
            return Ok(None);
        }

        let payload: serde_json::Value = response
            .json()
            .await
            .map_err(|error| AppError::github(error.to_string()))?;
        installation_id = payload.get("id").and_then(|value| value.as_i64());
        if let Some(installation_id) = installation_id {
            let pool = crate::error::require_db(&state.db)?;
            sqlx::query("UPDATE repos SET github_installation_id = $1 WHERE github_repo_id = $2")
                .bind(installation_id)
                .bind(github_repo_id)
                .execute(pool)
                .await
                .map_err(|error| AppError::database(error.to_string()))?;
            invalidate_repo_cache(state, repo.id, Some(github_repo_id)).await;
        }
    }

    let Some(installation_id) = installation_id else {
        return Ok(None);
    };

    let jwt = create_app_jwt(state).await?;
    let url = format!("https://api.github.com/app/installations/{installation_id}/access_tokens");
    let response = state
        .http_client
        .post(&url)
        .header("Authorization", format!("Bearer {jwt}"))
        .header("Accept", "application/vnd.github.v3+json")
        .header("User-Agent", "Trustless-OSS-Bot")
        .send()
        .await
        .map_err(|error| AppError::github(error.to_string()))?;

    if !response.status().is_success() {
        return Ok(None);
    }

    let payload: serde_json::Value = response
        .json()
        .await
        .map_err(|error| AppError::github(error.to_string()))?;
    let token = payload
        .get("token")
        .and_then(|value| value.as_str())
        .map(str::to_owned);

    if let Some(ref token) = token {
        state
            .cache
            .set(&cache_key, token, Some(cache_keys::GH_TOKEN_TTL))
            .await;
    }

    Ok(token)
}

pub async fn list_installation_repos(
    state: &AppState,
    installation_id: i64,
) -> Result<Vec<InstallationRepo>, AppError> {
    let jwt = create_app_jwt(state).await?;
    let token_url =
        format!("https://api.github.com/app/installations/{installation_id}/access_tokens");
    let token = state
        .http_client
        .post(&token_url)
        .header("Authorization", format!("Bearer {jwt}"))
        .header("Accept", "application/vnd.github.v3+json")
        .header("User-Agent", "Trustless-OSS-Bot")
        .send()
        .await
        .map_err(|error| AppError::github(error.to_string()))?
        .error_for_status()
        .map_err(|error| AppError::github(error.to_string()))?
        .json::<InstallationTokenResponse>()
        .await
        .map_err(|error| AppError::github(error.to_string()))?
        .token;

    let response = state
        .http_client
        .get("https://api.github.com/installation/repositories?per_page=100")
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github.v3+json")
        .header("User-Agent", "Trustless-OSS-Bot")
        .send()
        .await
        .map_err(|error| AppError::github(error.to_string()))?
        .error_for_status()
        .map_err(|error| AppError::github(error.to_string()))?
        .json::<InstallationReposResponse>()
        .await
        .map_err(|error| AppError::github(error.to_string()))?;

    Ok(response.repositories)
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
        let mut guard = COMMENT_CACHE.lock().await;
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
        *guard = Some((cache_key, Instant::now()));
    }

    let pool = crate::error::require_db(&state.db)?;
    let github_repo_id: Option<i64> =
        sqlx::query_scalar("SELECT github_repo_id FROM repos WHERE full_name = $1")
            .bind(full_name)
            .fetch_optional(pool)
            .await
            .map_err(|error| AppError::database(error.to_string()))?;

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
        .header("Accept", "application/vnd.github.v3+json")
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({ "body": body }))
        .send()
        .await
        .map_err(|error| AppError::github(error.to_string()))?;

    if !response.status().is_success() {
        error!(
            repo = full_name,
            issue = issue_number,
            status = %response.status(),
            "failed to post comment"
        );
    }

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
        .header("Accept", "application/vnd.github.v3+json")
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
        .header("Accept", "application/vnd.github.v3+json")
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
