use crate::api::AuthHeader;
use crate::error::AppError;
use crate::utils::jwt::Claims;
use crate::utils::repo_identifier::identifier_from_full_name;
use crate::utils::state::AppState;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::IntoResponse;
use std::sync::Arc;

fn admin_claims(state: &AppState) -> Claims {
    Claims {
        sub: state.config.default_user.clone(),
        exp: 0,
        iss: None,
        iat: None,
    }
}

pub async fn require_authentication(
    State(state): State<Arc<AppState>>,
    _auth: Option<AuthHeader>,
    mut req: Request,
    next: Next,
) -> Result<impl IntoResponse, AppError> {
    req.extensions_mut().insert(admin_claims(&state));
    Ok(next.run(req).await)
}

pub async fn populate_oci_claims(
    State(state): State<Arc<AppState>>,
    _auth: Option<AuthHeader>,
    mut req: Request,
    next: Next,
) -> Result<impl IntoResponse, AppError> {
    req.extensions_mut().insert(admin_claims(&state));
    Ok(next.run(req).await)
}

pub async fn authorize_repository_access(
    State(_state): State<Arc<AppState>>,
    _auth: Option<AuthHeader>,
    mut req: Request,
    next: Next,
) -> Result<impl IntoResponse, AppError> {
    let Some(full_name) = extract_full_repo_name(req.uri().path()) else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };

    req.extensions_mut()
        .insert(identifier_from_full_name(full_name));
    Ok(next.run(req).await)
}

fn extract_full_repo_name(url: &str) -> Option<String> {
    let mut segments: Vec<&str> = url.split("/").filter(|s| !s.is_empty()).collect();
    if segments.first() == Some(&"v2") {
        segments.remove(0);
    } else if segments.as_slice().starts_with(&["api", "v1"]) {
        segments.drain(..2);
    }
    match segments.as_slice() {
        // tail: /{name}/manifests/{reference}
        [name @ .., "manifests", _reference] if !name.is_empty() => Some(name.join("/")),
        // tail: /{name}/blobs/{digest}
        [name @ .., "blobs", digest] if !name.is_empty() && *digest != "uploads" => {
            Some(name.join("/"))
        }
        // tail: /{name}/blobs/uploads/
        [name @ .., "blobs", "uploads"] if !name.is_empty() => Some(name.join("/")),
        // tail: /{name}/blobs/uploads/{session_id}
        [name @ .., "blobs", "uploads", _] if !name.is_empty() => Some(name.join("/")),
        // tail: /{name}/tags/list
        [name @ .., "tags", "list"] if !name.is_empty() => Some(name.join("/")),
        // tail: /{name}/referrers/{digest}
        [name @ .., "referrers", _digest] if !name.is_empty() => Some(name.join("/")),
        // tail: /{name}/visibility
        [name @ .., "visibility"] if !name.is_empty() => Some(name.join("/")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::extract_full_repo_name;

    #[test]
    fn extract_repo_name_from_manifest_path() {
        assert_eq!(
            extract_full_repo_name("/v2/admin/app/manifests/latest"),
            Some("admin/app".to_string())
        );
    }

    #[test]
    fn extract_repo_name_rejects_unknown_path() {
        assert_eq!(extract_full_repo_name("/v2/"), None);
    }
}
