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
    pub github_id: i64,
    pub github_username: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SupabaseUser {
    id: String,
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
}

#[derive(Debug, Default, Deserialize)]
struct SupabaseIdentity {
    provider: Option<String>,
    id: Option<ProviderId>,
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
            let token = authorization
                .and_then(|value| value.to_str().ok().map(str::to_owned))
                .and_then(|value| bearer_token(&value).map(str::to_owned))
                .ok_or_else(unauthorized_response)?;

            let user_url = format!("{}/auth/v1/user", config.supabase_url.trim_end_matches('/'));
            let user = client
                .get(user_url)
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header("apikey", &config.supabase_auth_api_key)
                .send()
                .await
                .map_err(|error| {
                    error!(%error, "failed to verify token with Supabase Auth");
                    unauthorized_response()
                })?;

            if user.status() != StatusCode::OK {
                error!(
                    status = %user.status(),
                    "Supabase Auth rejected bearer token"
                );
                return Err(unauthorized_response());
            }

            let user = user.json::<SupabaseUser>().await.map_err(|error| {
                error!(%error, "failed to parse Supabase Auth user response");
                unauthorized_response()
            })?;

            let github_id = github_id(&user).ok_or_else(unauthorized_response)?;
            let github_username = github_username(&user);

            Ok(Self {
                github_id,
                github_username,
            })
        }
    }
}

fn bearer_token(value: &str) -> Option<&str> {
    let (scheme, token) = value.split_once(' ')?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then_some(token.trim())
}

fn github_id(user: &SupabaseUser) -> Option<i64> {
    user.user_metadata
        .provider_id
        .as_ref()
        .and_then(ProviderId::as_i64)
        .or_else(|| user.user_metadata.sub.as_ref().and_then(ProviderId::as_i64))
        .or_else(|| {
            user.identities
                .iter()
                .find(|identity| identity.provider.as_deref() == Some("github"))
                .and_then(|identity| {
                    identity
                        .identity_data
                        .provider_id
                        .as_ref()
                        .or(identity.id.as_ref())
                        .and_then(ProviderId::as_i64)
                })
        })
        .or_else(|| user.id.parse().ok())
}

fn github_username(user: &SupabaseUser) -> Option<String> {
    user.user_metadata
        .user_name
        .clone()
        .or_else(|| user.user_metadata.preferred_username.clone())
        .or_else(|| {
            user.identities
                .iter()
                .find(|identity| identity.provider.as_deref() == Some("github"))
                .and_then(|identity| {
                    identity
                        .identity_data
                        .user_name
                        .clone()
                        .or_else(|| identity.identity_data.preferred_username.clone())
                })
        })
}
