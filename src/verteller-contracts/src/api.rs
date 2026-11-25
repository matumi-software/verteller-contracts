//! Problem JSON errors, with the codes from `docs/api.md`.

use axum::{
    Json,
    extract::multipart::MultipartRejection,
    extract::rejection::{JsonRejection, PathRejection, QueryRejection},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;

const PROBLEM_BASE: &str = "https://matumi.dev/verteller/problems/";

/// An error the API answers with, as `application/problem+json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    /// The HTTP status.
    pub status: StatusCode,
    /// The machine readable code from the table in `docs/api.md`.
    pub code: &'static str,
    /// A sentence for a person.
    pub detail: String,
    /// Seconds for a `Retry-After` header, for `429 too_many_attempts`.
    pub retry_after: Option<u64>,
}

#[derive(Serialize)]
struct Problem<'a> {
    #[serde(rename = "type")]
    kind: String,
    title: &'a str,
    status: u16,
    detail: &'a str,
    code: &'a str,
}

impl ApiError {
    /// An error with any status and code.
    pub fn new(status: StatusCode, code: &'static str, detail: impl Into<String>) -> Self {
        Self {
            status,
            code,
            detail: detail.into(),
            retry_after: None,
        }
    }

    /// `401 unauthorized`.
    pub fn unauthorized() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "Sign in, or send the app's token as `Authorization: Bearer <token>`.",
        )
    }

    /// `402 needs_pro`: the feature is part of Verteller Pro, and this build or licence does not
    /// have it.
    pub fn needs_pro(feature: crate::Feature) -> Self {
        Self::new(
            StatusCode::PAYMENT_REQUIRED,
            "needs_pro",
            format!("{} needs Verteller Pro.", feature.label()),
        )
    }

    /// `403 owner_only`: only the owner changes this.
    pub fn owner_only() -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "owner_only",
            "Only the owner of this Verteller can do that.",
        )
    }

    /// `403 host_only`: this is done on the computer that runs the app.
    pub fn host_only() -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "host_only",
            "Do this on the computer that runs Verteller, opened with `verteller open`.",
        )
    }

    /// `403 session_required`: the bearer token has no sign-in to change.
    pub fn session_required() -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "session_required",
            "This needs a signed-in browser. The app's token has no sign-in of its own.",
        )
    }

    /// `404 person_not_found`.
    pub fn person_not_found() -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "person_not_found",
            "There is no such person.",
        )
    }

    /// `429 too_many_attempts`, with the seconds to wait.
    pub fn too_many_attempts(retry_after: u64) -> Self {
        Self {
            retry_after: Some(retry_after),
            ..Self::new(
                StatusCode::TOO_MANY_REQUESTS,
                "too_many_attempts",
                format!("Too many tries. Wait {retry_after} seconds and try again."),
            )
        }
    }

    /// `403 forbidden_host`.
    pub fn forbidden_host() -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "forbidden_host",
            "The Host header is not the address the app is bound to.",
        )
    }

    /// `403 forbidden_origin`: a change sent from a page of another site.
    pub fn forbidden_origin() -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "forbidden_origin",
            "The Origin header is not this app's own address.",
        )
    }

    /// `403 missing_ui_header`.
    pub fn missing_ui_header() -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "missing_ui_header",
            "A request that changes something needs the header `X-Verteller-UI: 1`.",
        )
    }

    /// `400 invalid_request`: the body, query or path could not be read.
    pub fn invalid_request(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_request", detail)
    }

    /// `404 not_found`: no such route.
    pub fn not_found() -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "There is no such route.",
        )
    }

    /// `405 method_not_allowed`.
    pub fn method_not_allowed() -> Self {
        Self::new(
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
            "That method is not allowed on this route.",
        )
    }

    /// `404 voice_not_found`.
    pub fn voice_not_found(name: &str) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "voice_not_found",
            format!("There is no voice called {:?}.", crate::echo(name)),
        )
    }

    /// `409 audiocpp_unsupported`: this computer or build cannot use the audio.cpp download.
    pub fn audiocpp_unsupported(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, "audiocpp_unsupported", detail)
    }

    /// `500 internal_error`.
    pub fn internal(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", detail)
    }

    fn title(&self) -> &'static str {
        match self.code {
            "unauthorized" => "Unauthorized",
            "forbidden_host" => "Forbidden host",
            "forbidden_origin" => "Forbidden origin",
            "body_too_large" => "Body too large",
            "password_required" => "Password required",
            "port_in_use" => "Port in use",
            "missing_ui_header" => "Missing UI header",
            "invalid_voice_name" => "Invalid voice name",
            "invalid_display_name" => "Invalid display name",
            "consent_incomplete" => "Consent incomplete",
            "reference_too_short" => "Recording too short",
            "invalid_recording" => "Invalid recording",
            "script_empty" => "Script empty",
            "invalid_script" => "Invalid script",
            "invalid_pause" => "Invalid pause",
            "invalid_tag" => "Invalid tag",
            "script_too_large" => "Script too large",
            "voice_not_found" => "Voice not found",
            "job_not_found" => "Job not found",
            "segment_not_found" => "Segment not found",
            "voice_exists" => "Voice exists",
            "model_not_ready" => "Model not ready",
            "unknown_model" => "Unknown model",
            "consent_required" => "Consent required",
            "audiocpp_unsupported" => "audio.cpp files not available",
            "invalid_setting" => "Invalid setting",
            "model_not_installed" => "Model not installed",
            "startup_failed" => "Start at sign-in failed",
            "model_in_use" => "Model in use",
            "model_is_default" => "Model is the default",
            "download_busy" => "Download busy",
            "not_downloading" => "Not downloading",
            "job_finished" => "Job finished",
            "job_running" => "Job running",
            "job_not_done" => "Job not done",
            "not_found" => "Not found",
            "method_not_allowed" => "Method not allowed",
            "internal_error" => "Internal error",
            "integration_unsupported" => "Claude Desktop not available",
            "integration_config_invalid" => "Claude Desktop config not usable",
            "not_cleaned" => "No cleaned copy",
            "cleanup_busy" => "Clean-up busy",
            "cleanup_not_found" => "Clean-up not found",
            "owner_only" => "Owner only",
            "host_only" => "On the host only",
            "session_required" => "Sign-in required",
            "invalid_credentials" => "Wrong user name or password",
            "wrong_password" => "Wrong password",
            "invalid_code" => "Code not valid",
            "user_name_taken" => "User name taken",
            "invalid_user_name" => "Invalid user name",
            "password_too_short" => "Password too short",
            "password_already_set" => "Password already set",
            "cannot_remove_self" => "Cannot remove yourself",
            "person_not_found" => "Person not found",
            "device_not_found" => "Device not found",
            "invitation_not_found" => "No invitation",
            "reset_not_found" => "No reset code",
            "too_many_attempts" => "Too many attempts",
            "request_cancelled" => "Request cancelled",
            "own_account_only" => "Own account only",
            "too_many_wrong_passwords" => "Signed out after wrong passwords",
            _ => "Bad request",
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Problem {
            kind: format!("{PROBLEM_BASE}{}", self.code),
            title: self.title(),
            status: self.status.as_u16(),
            detail: &self.detail,
            code: self.code,
        };
        let bytes = serde_json::to_vec(&body).unwrap_or_default();
        let mut resp = (
            self.status,
            [(header::CONTENT_TYPE, "application/problem+json")],
            bytes,
        )
            .into_response();
        if let Some(wait) = self.retry_after {
            resp.headers_mut()
                .insert(header::RETRY_AFTER, axum::http::HeaderValue::from(wait));
        }
        resp
    }
}

impl From<crate::Error> for ApiError {
    fn from(e: crate::Error) -> Self {
        use crate::Error as E;
        let detail = e.to_string();
        match e {
            E::InvalidName(_) => Self::new(StatusCode::BAD_REQUEST, "invalid_voice_name", detail),
            E::InvalidDisplayName(_) => {
                Self::new(StatusCode::BAD_REQUEST, "invalid_display_name", detail)
            }
            E::Consent(_) => Self::new(StatusCode::BAD_REQUEST, "consent_incomplete", detail),
            E::ReferenceTooShort { .. } => {
                Self::new(StatusCode::BAD_REQUEST, "reference_too_short", detail)
            }
            E::ReferenceTooLong { .. } => {
                Self::new(StatusCode::BAD_REQUEST, "reference_too_long", detail)
            }
            E::UnknownModel(_) => Self::new(StatusCode::BAD_REQUEST, "unknown_model", detail),
            E::InvalidSetting(_) => Self::new(StatusCode::BAD_REQUEST, "invalid_setting", detail),
            E::Decode(_) => Self::new(StatusCode::BAD_REQUEST, "invalid_recording", detail),
            E::Wav(_) => Self::new(StatusCode::BAD_REQUEST, "invalid_recording", detail),
            E::EmptyScript => Self::new(StatusCode::BAD_REQUEST, "script_empty", detail),
            E::InvalidScript(_) => Self::new(StatusCode::BAD_REQUEST, "invalid_script", detail),
            E::NotCleaned(_) => Self::new(StatusCode::CONFLICT, "not_cleaned", detail),
            E::NoDraft(_) => Self::new(StatusCode::NOT_FOUND, "draft_not_found", detail),
            E::DraftOutOfDate(_) => Self::new(StatusCode::CONFLICT, "draft_out_of_date", detail),
            E::ProfileExists(_) => Self::new(StatusCode::CONFLICT, "voice_exists", detail),
            E::DisplayNameTaken(_) => Self::new(StatusCode::CONFLICT, "voice_name_taken", detail),
            E::ProfileNotFound(_) => Self::new(StatusCode::NOT_FOUND, "voice_not_found", detail),
            E::Cancelled => Self::new(StatusCode::CONFLICT, "job_cancelled", detail),
            E::OutOfMemory(_) => {
                tracing::error!("{detail}");
                Self::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "gpu_out_of_memory",
                    detail,
                )
            }
            E::Io(_) | E::Json(_) | E::Backend(_) => {
                tracing::error!("{detail}");
                Self::internal(detail)
            }
        }
    }
}

impl From<JsonRejection> for ApiError {
    fn from(r: JsonRejection) -> Self {
        if r.status() == StatusCode::PAYLOAD_TOO_LARGE {
            return Self::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "body_too_large",
                "The request body is larger than this route takes.",
            );
        }
        Self::invalid_request(r.body_text())
    }
}

impl From<QueryRejection> for ApiError {
    fn from(r: QueryRejection) -> Self {
        Self::invalid_request(r.body_text())
    }
}

impl From<MultipartRejection> for ApiError {
    fn from(r: MultipartRejection) -> Self {
        Self::invalid_request(r.body_text())
    }
}

impl From<PathRejection> for ApiError {
    fn from(r: PathRejection) -> Self {
        Self::invalid_request(r.body_text())
    }
}

/// A request body that is JSON, with a problem JSON answer when it is not.
#[derive(axum::extract::FromRequest)]
#[from_request(via(Json), rejection(ApiError))]
pub struct ApiJson<T>(pub T);

/// A multipart body, with a problem JSON answer when the request is not one.
pub struct ApiMultipart(pub axum::extract::Multipart);

impl<S: Send + Sync> axum::extract::FromRequest<S> for ApiMultipart {
    type Rejection = ApiError;

    async fn from_request(req: axum::extract::Request, state: &S) -> Result<Self, ApiError> {
        Ok(Self(
            axum::extract::Multipart::from_request(req, state).await?,
        ))
    }
}

/// A query string, with a problem JSON answer when it does not fit.
#[derive(axum::extract::FromRequestParts)]
#[from_request(via(axum::extract::Query), rejection(ApiError))]
pub struct ApiQuery<T>(pub T);

/// What a route returns.
pub type ApiResult<T> = Result<T, ApiError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_cancel_and_out_of_memory_have_codes() {
        let cancelled = ApiError::from(crate::Error::Cancelled);
        assert_eq!(cancelled.status, StatusCode::CONFLICT);
        assert_eq!(cancelled.code, "job_cancelled");

        let oom = ApiError::from(crate::Error::OutOfMemory("out of memory".into()));
        assert_eq!(oom.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(oom.code, "gpu_out_of_memory");
    }
}
