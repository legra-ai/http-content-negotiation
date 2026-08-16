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
pub use crate::error::NegotiationError;
pub use crate::language::{
    AcceptLanguage, LocalePolicy, LocaleRange, SelectedLocale, accept_language_from_headers,
};
pub use crate::layer::{ContentNegotiationLayer, DeferredResponse, RenderContext, RenderError};
pub use crate::media::{
    NegotiatedRepresentation, Representation, RepresentationId, RepresentationRegistry,
    RequestMediaType, RequestMediaTypes,
};
