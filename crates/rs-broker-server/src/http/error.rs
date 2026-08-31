//! Mapping from the internal `tonic::Status` error currency to HTTP error
//! responses shared by every REST handler.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use tonic::Status;

/// Client-facing error body: `{"error": {"code": "...", "message": "..."}}`.
#[derive(Debug, Serialize)]
pub struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Debug, Serialize)]
struct ErrorDetail {
    code: &'static str,
    message: String,
}

/// Map a `tonic::Status` (the shared internal error currency of the
/// `*_inner` methods) to an HTTP status code.
pub fn status_to_http(status: &Status) -> StatusCode {
    match status.code() {
        tonic::Code::InvalidArgument | tonic::Code::OutOfRange => StatusCode::BAD_REQUEST,
        tonic::Code::NotFound => StatusCode::NOT_FOUND,
        tonic::Code::AlreadyExists | tonic::Code::FailedPrecondition => StatusCode::CONFLICT,
        tonic::Code::Unauthenticated => StatusCode::UNAUTHORIZED,
        tonic::Code::PermissionDenied => StatusCode::FORBIDDEN,
        tonic::Code::Unimplemented => StatusCode::NOT_IMPLEMENTED,
        tonic::Code::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        tonic::Code::DeadlineExceeded => StatusCode::GATEWAY_TIMEOUT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// Human-readable gRPC-style code string carried in the error body.
fn status_code_name(status: &Status) -> &'static str {
    match status.code() {
        tonic::Code::Ok => "ok",
        tonic::Code::Cancelled => "cancelled",
        tonic::Code::Unknown => "unknown",
        tonic::Code::InvalidArgument => "invalid_argument",
        tonic::Code::DeadlineExceeded => "deadline_exceeded",
        tonic::Code::NotFound => "not_found",
        tonic::Code::AlreadyExists => "already_exists",
        tonic::Code::PermissionDenied => "permission_denied",
        tonic::Code::ResourceExhausted => "resource_exhausted",
        tonic::Code::FailedPrecondition => "failed_precondition",
        tonic::Code::Aborted => "aborted",
        tonic::Code::OutOfRange => "out_of_range",
        tonic::Code::Unimplemented => "unimplemented",
        tonic::Code::Internal => "internal",
        tonic::Code::Unavailable => "unavailable",
        tonic::Code::DataLoss => "data_loss",
        tonic::Code::Unauthenticated => "unauthenticated",
    }
}

impl ErrorBody {
    pub fn from_status(status: &Status) -> Self {
        Self {
            error: ErrorDetail {
                code: status_code_name(status),
                message: status.message().to_string(),
            },
        }
    }
}

/// Convert an internal `Status` into an axum JSON error response.
pub fn status_response(status: Status) -> Response {
    let code = status_to_http(&status);
    let body = ErrorBody::from_status(&status);
    (code, Json(body)).into_response()
}

/// Axum extractor/response wrapper so handlers can `?` an internal
/// `tonic::Status` and get the shared JSON error mapping. The status is
/// boxed to keep the error variant small (`result_large_err`).
pub struct ApiError(Box<Status>);

impl ApiError {
    pub fn new(status: Status) -> Self {
        Self(Box::new(status))
    }
}

impl From<Status> for ApiError {
    fn from(status: Status) -> Self {
        Self(Box::new(status))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        status_response(*self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_argument_and_notfound_codes() {
        assert_eq!(
            status_to_http(&Status::invalid_argument("bad")),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            status_to_http(&Status::not_found("gone")),
            StatusCode::NOT_FOUND
        );
    }

    #[test]
    fn maps_conflict_family_codes() {
        assert_eq!(
            status_to_http(&Status::already_exists("dup")),
            StatusCode::CONFLICT
        );
        assert_eq!(
            status_to_http(&Status::failed_precondition("state")),
            StatusCode::CONFLICT
        );
    }

    #[test]
    fn maps_transport_and_server_codes() {
        assert_eq!(
            status_to_http(&Status::unimplemented("nope")),
            StatusCode::NOT_IMPLEMENTED
        );
        assert_eq!(
            status_to_http(&Status::unavailable("down")),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            status_to_http(&Status::deadline_exceeded("slow")),
            StatusCode::GATEWAY_TIMEOUT
        );
        assert_eq!(
            status_to_http(&Status::internal("boom")),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn error_body_carries_code_and_message() {
        let body = ErrorBody::from_status(&Status::not_found("msg-123"));
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["error"]["code"], "not_found");
        assert_eq!(json["error"]["message"], "msg-123");
    }
}
