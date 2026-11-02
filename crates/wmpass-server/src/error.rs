//! Errors as Bitwarden clients expect to read them.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

/// What a handler can fail with. Internal details are never sent to a client.
#[derive(Debug)]
pub enum ApiError {
    /// 400 with a message.
    Bad(&'static str),
    /// 401.
    Unauthorised,
    /// Not found. Also used for what exists but belongs to someone else, so that a request
    /// cannot tell the two apart.
    NotFound,
    /// 429.
    TooMany(u64),
    /// 500. The cause goes to the log only.
    Internal,
}

/// Shorthand.
pub type ApiResult<T> = Result<T, ApiError>;

impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        eprintln!("database error: {error}");
        Self::Internal
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Bad(message) => (StatusCode::BAD_REQUEST, message),
            Self::Unauthorised => (StatusCode::UNAUTHORIZED, "Unauthorized."),
            Self::NotFound => (StatusCode::NOT_FOUND, "Resource not found."),
            Self::TooMany(seconds) => {
                let body = Json(json!({"message": "Too many attempts. Try again later.", "object": "error"}));
                let mut response = (StatusCode::TOO_MANY_REQUESTS, body).into_response();
                if let Ok(value) = seconds.to_string().parse() {
                    response.headers_mut().insert(axum::http::header::RETRY_AFTER, value);
                }
                return response;
            }
            Self::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong."),
        };
        let body = json!({
            "message": message,
            "validationErrors": null,
            "exceptionMessage": null,
            "exceptionStackTrace": null,
            "innerExceptionMessage": null,
            "object": "error",
        });
        (status, Json(body)).into_response()
    }
}
