//! Axum and Tower content and language negotiation for streaming responses.
//!
//! The crate selects a representation from HTTP request headers and exposes
//! the result through typed request extensions. It does not serialize payloads
//! or buffer response bodies. Applications provide the streaming encoder for
//! the selected representation.
#![doc = include_str!("../README.md")]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::return_self_not_must_use)]

mod accept;
mod error;
mod language;
mod layer;
mod media;

pub use crate::accept::{MediaRange, ParsedAccept};
pub use crate::error::{HeaderField, NegotiationError};
pub use crate::language::{
    AcceptLanguage, LanguageRange, LocalePolicy, LocaleRange, SelectedLocale,
    accept_language_from_headers,
};
pub use crate::layer::{ContentNegotiationLayer, DeferredResponse, RenderContext, RenderError};
pub use crate::media::{
    NegotiatedRepresentation, Representation, RepresentationId, RepresentationRegistry,
    RequestMediaType, RequestMediaTypes,
};
pub use unic_langid::{LanguageIdentifier, langid_slice};

/// Common media-type constants for representations frequently used with the
/// negotiation layer. Applications may register any additional media type.
pub mod media_type {
    /// JSON document representation.
    pub const APPLICATION_JSON: &str = "application/json";
    /// Newline-delimited JSON record-stream representation.
    pub const APPLICATION_NDJSON: &str = "application/x-ndjson";
    /// YAML document representation.
    pub const APPLICATION_YAML: &str = "application/yaml";
    /// Opaque byte-stream representation.
    pub const APPLICATION_OCTET_STREAM: &str = "application/octet-stream";
    /// UTF-8 plain-text error representation.
    pub const TEXT_PLAIN_UTF8: &str = "text/plain; charset=utf-8";
}
