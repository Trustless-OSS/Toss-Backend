use tokio::task::JoinSet;
use tracing::{debug, info};

use crate::{
    error::AppError, modules::github::auth::get_installation_token,
    shared::constants::DEFAULT_GITHUB_REPOSITORY_LABELS, state::AppState,
};

pub use crate::modules::github::auth::{list_github_labels, GitHubLabel};

const GITHUB_ACCEPT: &str = "application/vnd.github+json";
const GITHUB_API_VERSION: &str = "2022-11-28";
const GITHUB_USER_AGENT: &str = "Trustless-OSS-Bot";

pub async fn ensure_default_repository_labels(
    state: &AppState,
    github_repo_id: i64,
    full_name: &str,
) -> Result<(), AppError> {
    let existing = list_github_labels(state, github_repo_id, full_name).await?;
    let token = installation_token(state, github_repo_id, "create labels").await?;
    let mut label_tasks = JoinSet::new();

    for definition in DEFAULT_GITHUB_REPOSITORY_LABELS {
        if existing
            .iter()
            .any(|label| label.name.eq_ignore_ascii_case(definition.name))
        {
            continue;
        }

        let state = state.clone();
        let token = token.clone();
        let full_name = full_name.to_string();
        label_tasks.spawn(async move {
            let result = create_github_label(&state, &token, &full_name, definition).await;
            (definition, result)
        });
    }

    while let Some(task) = label_tasks.join_next().await {
        let (definition, result) = task.map_err(|error| {
            AppError::internal(format!("default GitHub label task failed: {error}"))
        })?;

        match result? {
            LabelCreation::Created => {
                info!(
                    repo = full_name,
                    label = definition.name,
                    "created default GitHub label"
                );
            }
            LabelCreation::AlreadyExists => {
                debug!(
                    repo = full_name,
                    label = definition.name,
                    "default GitHub label already exists"
                );
            }
        }
    }

    Ok(())
}

enum LabelCreation {
    Created,
    AlreadyExists,
}

async fn create_github_label(
    state: &AppState,
    token: &str,
    full_name: &str,
    definition: crate::shared::constants::DefaultGithubLabel,
) -> Result<LabelCreation, AppError> {
    let url = format!("https://api.github.com/repos/{full_name}/labels");
    let response = state
        .http_client
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", GITHUB_ACCEPT)
        .header("X-GitHub-Api-Version", GITHUB_API_VERSION)
        .header("User-Agent", GITHUB_USER_AGENT)
        .json(&serde_json::json!({
            "name": definition.name,
            "color": definition.color,
            "description": definition.description,
        }))
        .send()
        .await
        .map_err(|error| AppError::github(format!("create GitHub label failed: {error}")))?;

    let status = response.status();
    if status.is_success() {
        return Ok(LabelCreation::Created);
    }
    let text = response.text().await.unwrap_or_default();
    if status.as_u16() == 422 && label_already_exists(&text) {
        return Ok(LabelCreation::AlreadyExists);
    }

    Err(AppError::github(format!(
        "create label {} in {full_name} failed with {status}: {text}",
        definition.name
    )))
}

pub async fn delete_github_label(
    state: &AppState,
    github_repo_id: i64,
    full_name: &str,
    label_name: &str,
) -> Result<(), AppError> {
    let token = installation_token(state, github_repo_id, "delete label").await?;
    let url = format!(
        "https://api.github.com/repos/{full_name}/labels/{}",
        encode_label_name(label_name)
    );
    let response = state
        .http_client
        .delete(url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", GITHUB_ACCEPT)
        .header("X-GitHub-Api-Version", GITHUB_API_VERSION)
        .header("User-Agent", GITHUB_USER_AGENT)
        .send()
        .await
        .map_err(|error| AppError::github(format!("delete GitHub label failed: {error}")))?;

    // Deleting an already absent label is safe and makes the operation idempotent.
    if response.status().is_success() || response.status().as_u16() == 404 {
        return Ok(());
    }

    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    Err(AppError::github(format!(
        "delete label {label_name} in {full_name} failed with {status}: {text}"
    )))
}

async fn installation_token(
    state: &AppState,
    github_repo_id: i64,
    operation: &str,
) -> Result<String, AppError> {
    get_installation_token(state, github_repo_id)
        .await?
        .ok_or_else(|| {
            AppError::github(format!("failed to get installation token for {operation}"))
        })
}

fn encode_label_name(label_name: &str) -> String {
    label_name
        .chars()
        .flat_map(|character| {
            if character.is_alphanumeric() || matches!(character, '-' | '_' | '.') {
                vec![character]
            } else {
                format!("%{:02X}", character as u32).chars().collect()
            }
        })
        .collect()
}

fn label_already_exists(response_body: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(response_body)
        .ok()
        .and_then(|body| {
            body.get("errors")
                .and_then(serde_json::Value::as_array)
                .cloned()
        })
        .is_some_and(|errors| {
            errors.iter().any(|error| {
                error.get("resource").and_then(serde_json::Value::as_str) == Some("Label")
                    && error.get("field").and_then(serde_json::Value::as_str) == Some("name")
                    && error.get("code").and_then(serde_json::Value::as_str)
                        == Some("already_exists")
            })
        })
}

#[cfg(test)]
mod tests {
    use super::{encode_label_name, label_already_exists};

    #[test]
    fn encodes_label_names_for_a_github_url() {
        assert_eq!(encode_label_name("needs review"), "needs%20review");
    }

    #[test]
    fn recognizes_only_githubs_duplicate_label_error() {
        let duplicate = r#"{
            "errors": [{"resource": "Label", "field": "name", "code": "already_exists"}]
        }"#;

        assert!(label_already_exists(duplicate));
        assert!(!label_already_exists(r#"{"message": "Validation Failed"}"#));
    }
}
