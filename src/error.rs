use axum::http::StatusCode;
use std::fmt::{Display, Formatter};
use thiserror::Error;

/// An HTTP request header used by the negotiation layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderField {
    /// The `Accept` request header.
    Accept,
    /// The `Content-Type` request header.
    ContentType,
    /// The `Accept-Language` request header.
    AcceptLanguage,
}

impl HeaderField {
    /// The lowercase wire name of this header.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accept => "accept",
            Self::ContentType => "content-type",
            Self::AcceptLanguage => "accept-language",
        }
    }
}

impl Display for HeaderField {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Errors raised while parsing request headers or selecting a representation.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NegotiationError {
    /// A header value was not valid UTF-8 or contained invalid syntax.
    #[error("invalid {name} header: {detail}")]
    InvalidHeader {
        /// The HTTP header that failed validation.
        name: HeaderField,
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
    pub(crate) fn invalid_header(name: HeaderField, detail: impl Into<String>) -> Self {
        Self::InvalidHeader {
            name,
            detail: detail.into(),
        }
    }

    pub(crate) fn status(&self) -> StatusCode {
        match self {
            Self::InvalidHeader {
                name: HeaderField::ContentType,
                ..
            }
            | Self::UnsupportedMediaType { .. } => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::InvalidHeader { .. } => StatusCode::BAD_REQUEST,
            Self::NotAcceptable { .. } => StatusCode::NOT_ACCEPTABLE,
        }
    }
}
