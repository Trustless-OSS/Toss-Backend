//! Authorization from GitHub-owned identities and permissions, never request metadata.
use crate::{error::AppError, state::AppState};
use serde_json::Value;

pub(super) fn client() -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|_| AppError::internal("Unable to build authorization client"))
}

async fn get(client: &reqwest::Client, token: &str, path: &str) -> Result<Value, AppError> {
    let response = client
        .get(format!("https://api.github.com{path}"))
        .bearer_auth(token)
        .header("User-Agent", "Trustless-OSS-Bot")
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .map_err(|_| AppError::github("GitHub authorization lookup failed"))?;
    if !response.status().is_success() {
        return Err(AppError::forbidden(
            "GitHub could not verify repository administrator access",
        ));
    }
    response
        .json()
        .await
        .map_err(|_| AppError::github("Invalid GitHub authorization response"))
}

fn valid_segment(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

pub fn valid_repo_name(name: &str) -> bool {
    let parts: Vec<_> = name.split('/').collect();
    parts.len() == 2 && parts.into_iter().all(valid_segment)
}

fn verified_login(user: &Value, id: i64) -> Result<String, AppError> {
    if id <= 0 || user["id"].as_i64() != Some(id) {
        return Err(AppError::forbidden(
            "GitHub token or identity does not belong to the authenticated user",
        ));
    }
    user["login"]
        .as_str()
        .filter(|s| valid_segment(s))
        .map(str::to_owned)
        .ok_or_else(|| AppError::forbidden("GitHub identity is missing"))
}

fn validate_connect(
    user: &Value,
    repo: &Value,
    user_id: i64,
    repo_id: i64,
    full_name: &str,
) -> Result<String, AppError> {
    let login = verified_login(user, user_id)?;
    if repo_id <= 0
        || repo["id"].as_i64() != Some(repo_id)
        || !repo["full_name"]
            .as_str()
            .is_some_and(|name| name.eq_ignore_ascii_case(full_name))
        || repo["private"].as_bool() != Some(false)
        || repo["fork"].as_bool() != Some(false)
        || repo["permissions"]["admin"].as_bool() != Some(true)
    {
        return Err(AppError::forbidden(
            "Verified public, non-fork repository administrator access is required",
        ));
    }
    Ok(login)
}

pub async fn verify_connect(
    token: &str,
    user_id: i64,
    repo_id: i64,
    full_name: &str,
) -> Result<(String, String), AppError> {
    if !valid_repo_name(full_name) {
        return Err(AppError::bad_request("Invalid repository name"));
    }
    let client = client()?;
    let user = get(&client, token, "/user").await?;
    verified_login(&user, user_id)?;
    let repo = get(&client, token, &format!("/repos/{full_name}")).await?;
    let login = validate_connect(&user, &repo, user_id, repo_id, full_name)?;
    Ok((login, repo["full_name"].as_str().unwrap().to_owned()))
}

fn is_admin(permission: &Value, id: i64) -> bool {
    id > 0
        && permission["user"]["id"].as_i64() == Some(id)
        && permission["permission"].as_str() == Some("admin")
}

pub async fn require_installation_repo_admin(
    state: &AppState,
    installation: i64,
    user_id: i64,
    full_name: &str,
) -> Result<String, AppError> {
    let (login, permitted) =
        installation_repo_admins(state, installation, user_id, &[full_name.to_owned()]).await?;
    if !permitted.contains(full_name) {
        return Err(AppError::forbidden(
            "Repository administrator access is required for this installation",
        ));
    }
    Ok(login)
}

/// Look up the installation token and caller once, then check repository roles
/// with bounded concurrent requests. A 404 means this caller cannot administer
/// that repository; other GitHub failures abort the entire operation.
pub async fn installation_repo_admins(
    state: &AppState,
    installation: i64,
    user_id: i64,
    names: &[String],
) -> Result<(String, std::collections::HashSet<String>), AppError> {
    if installation <= 0 || user_id <= 0 || names.iter().any(|name| !valid_repo_name(name)) {
        return Err(AppError::bad_request(
            "Invalid installation, identity, or repository",
        ));
    }
    let token = super::auth::exchange_installation_token(state, installation).await?;
    let client = client()?;
    let user = get(&client, &token, &format!("/user/{user_id}")).await?;
    let login = verified_login(&user, user_id)?;
    let mut authorized = std::collections::HashSet::new();
    let mut pending = tokio::task::JoinSet::new();
    for full_name in names {
        let client = client.clone();
        let token = token.clone();
        let login = login.clone();
        let full_name = full_name.clone();
        pending.spawn(async move {
            let url = format!(
                "https://api.github.com/repos/{full_name}/collaborators/{login}/permission"
            );
            let response = client
                .get(url)
                .bearer_auth(&token)
                .header("User-Agent", "Trustless-OSS-Bot")
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .send()
                .await
                .map_err(|_| AppError::github("GitHub permission lookup failed"))?;
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok((full_name, false));
            }
            if !response.status().is_success() {
                return Err(AppError::github("GitHub permission lookup failed"));
            }
            let permission: Value = response
                .json()
                .await
                .map_err(|_| AppError::github("Invalid GitHub permission response"))?;
            Ok((full_name, is_admin(&permission, user_id)))
        });
        if pending.len() >= 8 {
            collect_permission(&mut pending, &mut authorized).await?;
        }
    }
    while !pending.is_empty() {
        collect_permission(&mut pending, &mut authorized).await?;
    }
    Ok((login, authorized))
}

async fn collect_permission(
    pending: &mut tokio::task::JoinSet<Result<(String, bool), AppError>>,
    authorized: &mut std::collections::HashSet<String>,
) -> Result<(), AppError> {
    let (name, is_admin) = pending
        .join_next()
        .await
        .ok_or_else(|| AppError::internal("Missing GitHub permission check"))?
        .map_err(|_| AppError::internal("GitHub permission check failed"))??;
    if is_admin {
        authorized.insert(name);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn connect_rejects_impersonation_and_repository_substitution() {
        let user = json!({"id":42,"login":"actual"});
        let repo = json!({"id":123,"full_name":"org/repo","private":false,"fork":false,"permissions":{"admin":true}});
        assert_eq!(
            validate_connect(&user, &repo, 42, 123, "org/repo").unwrap(),
            "actual"
        );
        assert!(validate_connect(&user, &repo, 999, 123, "org/repo").is_err());
        assert!(validate_connect(&user, &repo, 42, 456, "org/repo").is_err());
        assert!(validate_connect(&user, &repo, 42, 123, "other/repo").is_err());
        for field in ["private", "fork"] {
            let mut changed = repo.clone();
            changed[field] = json!(true);
            assert!(validate_connect(&user, &changed, 42, 123, "org/repo").is_err());
        }
        for permission in [json!(false), Value::Null] {
            let mut changed = repo.clone();
            changed["permissions"]["admin"] = permission;
            assert!(validate_connect(&user, &changed, 42, 123, "org/repo").is_err());
        }
    }
    #[test]
    fn installation_permission_requires_matching_numeric_identity_and_admin() {
        assert!(is_admin(
            &json!({"user":{"id":42},"permission":"admin"}),
            42
        ));
        assert!(!is_admin(
            &json!({"user":{"id":999},"permission":"admin"}),
            42
        ));
        for role in ["write", "maintain", "read", "none"] {
            assert!(!is_admin(&json!({"user":{"id":42},"permission":role}), 42));
        }
        assert!(!is_admin(&json!({"permission":"admin"}), 42));
    }
    #[test]
    fn repository_paths_cannot_inject_other_api_routes() {
        for name in [
            "../user",
            "org/..",
            "org/repo/hooks",
            "org/repo?x=1",
            "org/%2e%2e",
            "org\\repo",
        ] {
            assert!(!valid_repo_name(name));
        }
        assert!(valid_repo_name("org/my-repo.rs"));
    }
}
