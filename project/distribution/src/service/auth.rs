use crate::error::AppError;
use crate::utils::jwt::gen_token;
use crate::utils::state::AppState;
use axum::Json;
use axum::extract::State;
use axum::response::IntoResponse;
use axum_extra::TypedHeader;
use axum_extra::headers::Authorization;
use axum_extra::headers::authorization::Basic;
use chrono::Utc;
use serde::Serialize;
use std::sync::Arc;

fn canonical_namespace(value: &str) -> String {
    value.to_ascii_lowercase()
}

#[derive(Serialize)]
pub struct AuthResponse {
    token: String,
    #[serde(rename = "access_token")]
    access_token: String,
    #[serde(rename = "expires_in")]
    expires_in: i64,
    #[serde(rename = "issued_at")]
    issued_at: String,
}

pub(crate) async fn auth(
    State(state): State<Arc<AppState>>,
    _auth: Option<TypedHeader<Authorization<Basic>>>,
) -> Result<impl IntoResponse, AppError> {
    let token = gen_token(
        state.config.jwt_lifetime_secs,
        &state.config.jwt_secret,
        canonical_namespace(&state.config.default_user),
    );
    Ok(Json(AuthResponse {
        token: token.clone(),
        access_token: token,
        expires_in: state.config.jwt_lifetime_secs,
        issued_at: Utc::now().to_rfc3339(),
    }))
}

#[cfg(test)]
mod tests {
    use super::canonical_namespace;

    #[test]
    fn canonical_namespace_is_lowercase() {
        assert_eq!(canonical_namespace("LingBou"), "lingbou");
    }
}
