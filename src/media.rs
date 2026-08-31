use std::fmt;
use std::sync::Arc;

use axum::http::{HeaderMap, header};

use crate::accept::ParsedAccept;
use crate::error::{HeaderField, NegotiationError};

/// Why a media type was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidMediaType {
    reason: &'static str,
    value: String,
}

impl fmt::Display for InvalidMediaType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid media type: {} ({:?})", self.reason, self.value)
    }
}

impl std::error::Error for InvalidMediaType {}

/// A validated, concrete `type/subtype` media type — the one media-type
/// value shared across the negotiation family: representation
/// declarations here, response tables and `Content-Type` stamps in
/// boundary crates.
///
/// Limits: 1–255 bytes, exactly one `/` separating two non-empty
/// segments of HTTP token characters (RFC 9110 §5.6.2), no wildcards.
/// Parameters are not part of a `MediaType`; they belong to header
/// rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MediaType(&'static str);

impl MediaType {
    /// Validate and wrap a static media type.
    ///
    /// Declarations are built once from compile-time constants, so the
    /// value is `'static`; validation still runs so a malformed
    /// constant fails fast at construction.
    ///
    /// # Errors
    ///
    /// [`InvalidMediaType`] when the value is empty, longer than 255
    /// bytes, has no single `/`, contains a wildcard segment, or
    /// contains a non-token character.
    pub fn try_new(value: &'static str) -> Result<Self, InvalidMediaType> {
        let invalid = |reason| InvalidMediaType {
            reason,
            value: value.to_owned(),
        };
        if value.is_empty() {
            return Err(invalid("must not be empty"));
        }
        if value.len() > 255 {
            return Err(invalid("longer than 255 bytes"));
        }
        let Some((main, sub)) = value.split_once('/') else {
            return Err(invalid("must be type/subtype"));
        };
        if main.is_empty() || sub.is_empty() || sub.contains('/') {
            return Err(invalid("must be exactly two non-empty segments"));
        }
        if main == "*" || sub == "*" {
            return Err(invalid("wildcards are ranges, not concrete media types"));
        }
        if !Self::is_token(main) || !Self::is_token(sub) {
            return Err(invalid("segments must be HTTP token characters"));
        }
        Ok(Self(value))
    }

    /// Whether every byte of `segment` is an HTTP token character.
    fn is_token(segment: &str) -> bool {
        segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
    }

    /// Whether `value` is a concrete media type by this crate's rules
    /// (used to validate runtime `Content-Type` values with the same
    /// grammar as static declarations).
    #[must_use]
    pub fn is_valid(value: &str) -> bool {
        let Some((main, sub)) = value.split_once('/') else {
            return false;
        };
        !main.is_empty()
            && !sub.is_empty()
            && !sub.contains('/')
            && main != "*"
            && sub != "*"
            && value.len() <= 255
            && Self::is_token(main)
            && Self::is_token(sub)
    }

    /// Validate and wrap a static media type at compile time.
    ///
    /// For `const` declarations. An invalid value fails compilation
    /// when evaluated in `const` context; fallible call sites use
    /// [`MediaType::try_new`].
    ///
    /// # Panics
    ///
    /// Panics (at compile time in `const` context) when the value is
    /// not a concrete `type/subtype` of HTTP token characters.
    #[must_use]
    pub const fn from_static(value: &'static str) -> Self {
        let bytes = value.as_bytes();
        assert!(
            !bytes.is_empty() && bytes.len() <= 255,
            "invalid media type length"
        );
        let mut index = 0;
        let mut slash = usize::MAX;
        while index < bytes.len() {
            let byte = bytes[index];
            if byte == b'/' {
                assert!(slash == usize::MAX, "media type has more than one '/'");
                slash = index;
            } else {
                let token = byte.is_ascii_alphanumeric()
                    || matches!(
                        byte,
                        b'!' | b'#'
                            | b'$'
                            | b'%'
                            | b'&'
                            | b'\''
                            | b'*'
                            | b'+'
                            | b'-'
                            | b'.'
                            | b'^'
                            | b'_'
                            | b'`'
                            | b'|'
                            | b'~'
                    );
                assert!(token, "media type contains a non-token character");
            }
            index += 1;
        }
        assert!(
            slash != usize::MAX && slash != 0 && slash != bytes.len() - 1,
            "media type must be type/subtype"
        );
        let main_wildcard = slash == 1 && bytes[0] == b'*';
        let sub_wildcard = slash == bytes.len() - 2 && bytes[bytes.len() - 1] == b'*';
        assert!(
            !main_wildcard && !sub_wildcard,
            "wildcards are ranges, not concrete media types"
        );
        Self(value)
    }

    /// Validate and wrap a static media type that may be a wildcard
    /// range alias (`*/*`, `type/*`).
    ///
    /// Negotiation candidate tables may register a range alias to pin
    /// a wildcard `Accept` range to a specific representation at
    /// exact-match specificity. A range alias is never a valid
    /// `Content-Type` stamp — concrete stamps use
    /// [`MediaType::try_new`] / [`MediaType::from_static`].
    ///
    /// # Errors
    ///
    /// [`InvalidMediaType`] when the value is empty, longer than 255
    /// bytes, has no single `/`, or contains a non-token,
    /// non-wildcard character.
    pub fn try_new_range(value: &'static str) -> Result<Self, InvalidMediaType> {
        let invalid = |reason| InvalidMediaType {
            reason,
            value: value.to_owned(),
        };
        if value.is_empty() {
            return Err(invalid("must not be empty"));
        }
        if value.len() > 255 {
            return Err(invalid("longer than 255 bytes"));
        }
        let Some((main, sub)) = value.split_once('/') else {
            return Err(invalid("must be type/subtype"));
        };
        if main.is_empty() || sub.is_empty() || sub.contains('/') {
            return Err(invalid("must be exactly two non-empty segments"));
        }
        let segment_ok = |segment: &str| segment == "*" || Self::is_token(segment);
        if !segment_ok(main) || !segment_ok(sub) {
            return Err(invalid("segments must be HTTP token characters or '*'"));
        }
        if main == "*" && sub != "*" {
            return Err(invalid("'*/subtype' is not a valid media range"));
        }
        Ok(Self(value))
    }

    /// The validated media type.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for MediaType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl AsRef<str> for MediaType {
    fn as_ref(&self) -> &str {
        self.0
    }
}

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
    media_type: MediaType,
}

impl Representation {
    /// Construct a representation with an application-defined identifier and
    /// validated canonical media type.
    pub const fn new(id: RepresentationId, media_type: MediaType) -> Self {
        Self { id, media_type }
    }

    /// The application-defined representation identifier.
    pub const fn id(self) -> RepresentationId {
        self.id
    }

    /// The canonical response media type.
    pub const fn media_type(self) -> MediaType {
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
        let value = value.to_str().map_err(|_| {
            NegotiationError::invalid_header(HeaderField::ContentType, "value is not UTF-8")
        })?;
        let media_type = value.split(';').next().unwrap_or_default().trim();
        if !MediaType::is_valid(media_type) {
            return Err(NegotiationError::invalid_header(
                HeaderField::ContentType,
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
    accepted: Arc<[MediaType]>,
}

impl RequestMediaTypes {
    /// Construct the accepted request media type set from validated
    /// media types.
    pub fn new(accepted: impl IntoIterator<Item = MediaType>) -> Self {
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
            .any(|candidate| candidate.as_str().eq_ignore_ascii_case(media_type.as_str()))
        {
            Ok(Some(media_type))
        } else {
            Err(NegotiationError::UnsupportedMediaType {
                content_type: media_type.0,
            })
        }
    }
}
