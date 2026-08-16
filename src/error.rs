use axum::http::StatusCode;
use thiserror::Error;

/// Errors raised while parsing request headers or selecting a representation.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NegotiationError {
    /// A header value was not valid UTF-8 or contained invalid syntax.
    #[error("invalid {name} header: {detail}")]
    InvalidHeader {
        /// The HTTP header that failed validation.
        name: &'static str,
        /// The validation failure detail.
        detail: String,
    },
    /// No registered representation satisfies the `Accept` header.
    #[error("no acceptable representation for Accept header {accept:?}")]
    NotAcceptable {
        /// The original `Accept` header value.
        accept: String,
    },
    /// The request's `Content-Type` is not registered by the application.
    #[error("unsupported request Content-Type {content_type:?}")]
    UnsupportedMediaType {
        /// The original `Content-Type` value.
        content_type: String,
    },
}

impl NegotiationError {
    pub(crate) fn invalid_header(name: &'static str, detail: impl Into<String>) -> Self {
        Self::InvalidHeader {
            name,
            detail: detail.into(),
        }
    }

    pub(crate) fn status(&self) -> StatusCode {
        match self {
            Self::InvalidHeader { name, .. } if *name == "content-type" => {
                StatusCode::UNSUPPORTED_MEDIA_TYPE
            }
            Self::InvalidHeader { .. } => StatusCode::BAD_REQUEST,
            Self::NotAcceptable { .. } => StatusCode::NOT_ACCEPTABLE,
            Self::UnsupportedMediaType { .. } => StatusCode::UNSUPPORTED_MEDIA_TYPE,
        }
    }
}
