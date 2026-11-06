use axum::{
    extract::FromRequestParts,
    http::{header, request::Parts, StatusCode},
    response::Response,
};
use serde::Deserialize;
use tracing::error;

use crate::{error::unauthorized_response, state::AppState};

#[derive(Debug, Clone)]
pub struct AuthedUser {
    pub auth_user_id: uuid::Uuid,
    pub github_id: i64,
    pub github_username: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SupabaseUser {
    id: String,
    email: Option<String>,
    #[serde(default)]
    user_metadata: UserMetadata,
    #[serde(default)]
    identities: Vec<SupabaseIdentity>,
}

#[derive(Debug, Default, Deserialize)]
struct UserMetadata {
    provider_id: Option<ProviderId>,
    user_name: Option<String>,
    preferred_username: Option<String>,
    sub: Option<ProviderId>,
    email: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct SupabaseIdentity {
    provider: Option<String>,
    #[serde(default)]
    identity_data: UserMetadata,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum ProviderId {
    String(String),
    Number(i64),
}

impl ProviderId {
    fn as_i64(&self) -> Option<i64> {
        match self {
            Self::String(value) => value.parse().ok(),
            Self::Number(value) => Some(*value),
        }
    }
}

impl FromRequestParts<AppState> for AuthedUser {
    type Rejection = Response;

    fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> impl std::future::Future<Output = Result<Self, Self::Rejection>> + Send {
        let authorization = parts.headers.get(header::AUTHORIZATION).cloned();
        let config = state.config.clone();
        let client = state.http_client.clone();

        async move {
            if config.auth_bypass {
                return Err(unauthorized_response());
            }

            let token = authorization
                .and_then(|value| value.to_str().ok().map(str::to_owned))
                .and_then(|value| bearer_token(&value).map(str::to_owned))
                .ok_or_else(unauthorized_response)?;

            //  Here we send bearer token to auth service to verify the token
            let user_url = format!("{}/auth/v1/user", config.supabase_url.trim_end_matches('/'));
            let user = client
                .get(user_url)
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header("apikey", &config.supabase_auth_api_key)
                .send()
                .await
                .map_err(|error| {
                    error!(%error, "failed to verify token with Auth");
                    unauthorized_response()
                })?;

            if user.status() != StatusCode::OK {
                error!(
                    status = %user.status(),
                    "Auth rejected bearer token"
                );
                return Err(unauthorized_response());
            }

            let user = user.json::<SupabaseUser>().await.map_err(|error| {
                error!(%error, "failed to parse Auth user response");
                unauthorized_response()
            })?;

            let github_id = github_id(&user).ok_or_else(unauthorized_response)?;
            let auth_user_id =
                uuid::Uuid::parse_str(&user.id).map_err(|_| unauthorized_response())?;
            let github_username = github_username(&user);
            let email = github_email(&user);

            Ok(Self {
                auth_user_id,
                github_id,
                github_username,
                email,
            })
        }
    }
}

fn bearer_token(value: &str) -> Option<&str> {
    let (scheme, token) = value.split_once(' ')?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then_some(token.trim())
        .filter(|token| !token.is_empty())
}

fn github_identity(user: &SupabaseUser) -> Option<&SupabaseIdentity> {
    let mut identities = user
        .identities
        .iter()
        .filter(|identity| identity.provider.as_deref() == Some("github"));
    let identity = identities.next()?;
    if identities.next().is_some() {
        return None;
    }
    Some(identity)
}

fn github_id(user: &SupabaseUser) -> Option<i64> {
    let data = &github_identity(user)?.identity_data;
    let id = match (&data.provider_id, &data.sub) {
        (Some(provider), Some(sub)) => {
            let id = provider.as_i64()?;
            if sub.as_i64()? != id {
                return None;
            }
            id
        }
        (Some(id), None) | (None, Some(id)) => id.as_i64()?,
        (None, None) => return None,
    };
    (id > 0).then_some(id)
}

fn github_username(user: &SupabaseUser) -> Option<String> {
    let data = &github_identity(user)?.identity_data;
    data.user_name
        .clone()
        .or_else(|| data.preferred_username.clone())
}

fn github_email(user: &SupabaseUser) -> Option<String> {
    user.email
        .clone()
        .or_else(|| user.user_metadata.email.clone())
        .or_else(|| {
            user.identities
                .iter()
                .find(|i| i.provider.as_deref() == Some("github"))
                .and_then(|i| i.identity_data.email.clone())
        })
        .map(|e| e.trim().to_string())
        .filter(|e| !e.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn editable_metadata_cannot_change_authorization_identity() {
        let user = serde_json::from_value(json!({
            "id":"999", "user_metadata":{"provider_id":"999","sub":"999","user_name":"victim","preferred_username":"victim"},
            "identities":[{"provider":"github","identity_data":{"provider_id":"42","sub":42,"user_name":"actual"}}]
        })).unwrap();
        assert_eq!(github_id(&user), Some(42));
        assert_eq!(github_username(&user).as_deref(), Some("actual"));
    }

    #[test]
    fn missing_ambiguous_conflicting_and_invalid_identity_fail_closed() {
        for identities in [
            json!([]),
            json!([{"provider":"email","identity_data":{"provider_id":42}}]),
            json!([{"provider":"github","id":"42","identity_data":{}}]),
            json!([{"provider":"github","identity_data":{"provider_id":"42","sub":"999"}}]),
            json!([{"provider":"github","identity_data":{"provider_id":"bad","sub":"42"}}]),
            json!([{"provider":"github","identity_data":{"sub":0}}]),
            json!([{"provider":"github","identity_data":{"sub":-1}}]),
            json!([{"provider":"github","identity_data":{"sub":42}},{"provider":"github","identity_data":{"sub":42}}]),
        ] {
            let user = serde_json::from_value(
                json!({"id":"999","user_metadata":{"provider_id":999},"identities":identities}),
            )
            .unwrap();
            assert_eq!(github_id(&user), None);
        }
    }

    #[test]
    fn provider_subject_supports_both_numeric_and_string_ids() {
        for sub in [json!(42), json!("42")] {
            let user = serde_json::from_value(json!({"id":"uuid","identities":[{"provider":"github","identity_data":{"sub":sub}}]})).unwrap();
            assert_eq!(github_id(&user), Some(42));
        }
        assert_eq!(bearer_token("Bearer "), None);
    }
}
