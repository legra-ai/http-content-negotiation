use std::sync::Arc;

use axum::http::{HeaderMap, header};

use crate::accept::ParsedAccept;
use crate::error::NegotiationError;

/// Application-defined identifier for one wire representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RepresentationId(&'static str);

impl RepresentationId {
    /// Construct an identifier from a stable application-defined name.
    pub const fn new(name: &'static str) -> Self {
        Self(name)
    }

    /// Borrow the stable identifier name.
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

/// One registered response representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Representation {
    id: RepresentationId,
    media_type: &'static str,
}

impl Representation {
    /// Construct a representation with an application-defined identifier and
    /// canonical media type.
    pub const fn new(id: RepresentationId, media_type: &'static str) -> Self {
        Self { id, media_type }
    }

    /// The application-defined representation identifier.
    pub const fn id(self) -> RepresentationId {
        self.id
    }

    /// The canonical response media type.
    pub const fn media_type(self) -> &'static str {
        self.media_type
    }
}

/// A representation selected for the current request.
pub type NegotiatedRepresentation = Representation;

/// The response representation registry used by the Tower layer.
#[derive(Debug, Clone)]
pub struct RepresentationRegistry {
    default: Representation,
    // bounded: application configuration contains a finite representation set
    candidates: Arc<[Representation]>,
}

impl RepresentationRegistry {
    /// Construct a registry. Candidate order is the server preference used to
    /// break equal `Accept` quality and specificity ties.
    pub fn new(
        default: Representation,
        candidates: impl IntoIterator<Item = Representation>,
    ) -> Self {
        Self {
            default,
            candidates: candidates.into_iter().collect(),
        }
    }

    /// The representation used when `Accept` is absent or unconstrained.
    pub const fn default_representation(&self) -> Representation {
        self.default
    }

    /// The registered candidates in server preference order.
    pub fn candidates(&self) -> &[Representation] {
        &self.candidates
    }

    /// Negotiate a representation from an optional `Accept` value.
    ///
    /// # Errors
    ///
    /// Returns [`NegotiationError::InvalidHeader`] for malformed `Accept`
    /// values and [`NegotiationError::NotAcceptable`] when no candidate
    /// matches.
    pub fn negotiate(&self, accept: Option<&str>) -> Result<Representation, NegotiationError> {
        Ok(*ParsedAccept::negotiate_header(
            accept,
            &self.candidates,
            &self.default,
        )?)
    }
}

/// The request `Content-Type` media type, without parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestMediaType(String);

impl RequestMediaType {
    /// Parse the request `Content-Type`, returning `None` when absent.
    ///
    /// # Errors
    ///
    /// Returns [`NegotiationError::InvalidHeader`] when the header is not
    /// valid UTF-8 or does not contain a concrete media type.
    pub fn from_headers(headers: &HeaderMap) -> Result<Option<Self>, NegotiationError> {
        let Some(value) = headers.get(header::CONTENT_TYPE) else {
            return Ok(None);
        };
        let value = value
            .to_str()
            .map_err(|_| NegotiationError::invalid_header("content-type", "value is not UTF-8"))?;
        let media_type = value.split(';').next().unwrap_or_default().trim();
        if !is_concrete_media_type(media_type) {
            return Err(NegotiationError::invalid_header(
                "content-type",
                format!("invalid media type {media_type:?}"),
            ));
        }
        Ok(Some(Self(media_type.to_ascii_lowercase())))
    }

    /// Borrow the normalized media type.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A set of accepted request `Content-Type` media types.
#[derive(Debug, Clone)]
pub struct RequestMediaTypes {
    // bounded: application configuration contains a finite media type set
    accepted: Arc<[&'static str]>,
}

impl RequestMediaTypes {
    /// Construct the accepted request media type set.
    pub fn new(accepted: impl IntoIterator<Item = &'static str>) -> Self {
        Self {
            accepted: accepted.into_iter().collect(),
        }
    }

    /// Validate the request `Content-Type` against this set.
    ///
    /// # Errors
    ///
    /// Returns [`NegotiationError::UnsupportedMediaType`] when the supplied
    /// media type is not registered.
    pub fn validate(
        &self,
        media_type: Option<RequestMediaType>,
    ) -> Result<Option<RequestMediaType>, NegotiationError> {
        let Some(media_type) = media_type else {
            return Ok(None);
        };
        if self
            .accepted
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(media_type.as_str()))
        {
            Ok(Some(media_type))
        } else {
            Err(NegotiationError::UnsupportedMediaType {
                content_type: media_type.0,
            })
        }
    }
}

fn is_concrete_media_type(value: &str) -> bool {
    let Some((media_type, subtype)) = value.split_once('/') else {
        return false;
    };
    media_type != "*"
        && subtype != "*"
        && !media_type.is_empty()
        && !subtype.is_empty()
        && !value.bytes().any(|byte| byte.is_ascii_whitespace())
}
