use crate::error::{HeaderField, NegotiationError};
use crate::media::Representation;

/// One media range from an `Accept` header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaRange {
    media_type: String,
    quality_milli: u16,
}

impl MediaRange {
    fn parse(member: &str) -> Result<Option<Self>, NegotiationError> {
        let mut pieces = member.split(';');
        let media_type = pieces.next().unwrap_or_default().trim();
        if media_type.is_empty() {
            return Ok(None);
        }
        validate_media_range(media_type)?;

        let mut quality_milli = 1000;
        for parameter in pieces {
            let Some((key, value)) = parameter.trim().split_once('=') else {
                continue;
            };
            if key.trim().eq_ignore_ascii_case("q") {
                quality_milli = parse_quality_milli(value.trim())?;
            }
        }

        Ok(Some(Self {
            media_type: media_type.to_ascii_lowercase(),
            quality_milli,
        }))
    }

    /// The lowercase media range, such as `application/json` or `*/*`.
    pub fn media_type(&self) -> &str {
        &self.media_type
    }

    /// The quality value in thousandths (`q=0.5` is `500`).
    pub const fn quality_milli(&self) -> u16 {
        self.quality_milli
    }

    /// Return the RFC specificity of this range for a concrete candidate.
    ///
    /// Exact matches return `2`, type wildcards return `1`, and `*/*`
    /// returns `0`.
    pub fn specificity_for(&self, candidate: &str) -> Option<u8> {
        if self.media_type == candidate {
            return Some(2);
        }
        if self.media_type == "*/*" {
            return Some(0);
        }
        let type_prefix = self.media_type.strip_suffix("/*")?;
        let candidate_type = candidate.split('/').next().unwrap_or_default();
        (type_prefix == candidate_type).then_some(1)
    }
}

/// A parsed `Accept` header ready to negotiate against registered
/// representations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedAccept {
    // bounded: one HTTP header value is bounded by the server's header limit
    ranges: Vec<MediaRange>,
}

impl ParsedAccept {
    /// Parse an `Accept` header.
    ///
    /// Empty list members are ignored. Invalid media ranges and invalid
    /// quality values are rejected instead of silently selecting a fallback.
    ///
    /// # Errors
    ///
    /// Returns [`NegotiationError::InvalidHeader`] when a media range or
    /// quality value is malformed.
    pub fn parse(header: &str) -> Result<Self, NegotiationError> {
        let ranges = header
            .split(',')
            .map(MediaRange::parse)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect();
        Ok(Self { ranges })
    }

    /// Whether the header contained no media ranges.
    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    /// Select the best registered representation.
    ///
    /// Selection order is highest quality, highest specificity, then the
    /// registered candidate order. A matching `q=0` range excludes a
    /// candidate at that specificity.
    pub fn negotiate<'a>(&self, candidates: &'a [Representation]) -> Option<&'a Representation> {
        let mut best: Option<(u16, u8, usize)> = None;
        for (candidate_index, candidate) in candidates.iter().enumerate() {
            let mut candidate_best: Option<(u16, u8)> = None;
            for range in &self.ranges {
                if let Some(specificity) = range.specificity_for(candidate.media_type())
                    && candidate_best
                        .is_none_or(|(_, current_specificity)| specificity > current_specificity)
                {
                    candidate_best = Some((range.quality_milli(), specificity));
                }
            }
            let Some((quality, specificity)) = candidate_best else {
                continue;
            };
            if quality == 0 {
                continue;
            }
            if best.is_none_or(|(best_quality, best_specificity, _)| {
                (quality, specificity) > (best_quality, best_specificity)
            }) {
                best = Some((quality, specificity, candidate_index));
            }
        }
        best.map(|(_, _, index)| &candidates[index])
    }

    pub(crate) fn negotiate_header<'a>(
        accept: Option<&str>,
        candidates: &'a [Representation],
        default: &'a Representation,
    ) -> Result<&'a Representation, NegotiationError> {
        let Some(accept) = accept else {
            return Ok(default);
        };
        let parsed = Self::parse(accept)?;
        if parsed.is_empty() {
            return Ok(default);
        }
        parsed
            .negotiate(candidates)
            .ok_or_else(|| NegotiationError::NotAcceptable {
                accept: accept.to_owned(),
            })
    }
}

fn validate_media_range(value: &str) -> Result<(), NegotiationError> {
    let Some((media_type, subtype)) = value.split_once('/') else {
        return Err(NegotiationError::invalid_header(
            HeaderField::Accept,
            format!("invalid media range {value:?}"),
        ));
    };
    let valid_type = media_type == "*" || is_token(media_type);
    let valid_subtype = subtype == "*" || is_token(subtype);
    if !valid_type || !valid_subtype || (media_type == "*" && subtype != "*") {
        return Err(NegotiationError::invalid_header(
            HeaderField::Accept,
            format!("invalid media range {value:?}"),
        ));
    }
    Ok(())
}

fn parse_quality_milli(value: &str) -> Result<u16, NegotiationError> {
    let value = value.trim_matches('"');
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    let whole = whole.parse::<u16>().map_err(|_| {
        NegotiationError::invalid_header(HeaderField::Accept, format!("invalid q-value {value:?}"))
    })?;
    if whole > 1 || fraction.len() > 3 || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(NegotiationError::invalid_header(
            HeaderField::Accept,
            format!("invalid q-value {value:?}"),
        ));
    }
    let mut fraction_milli = fraction.parse::<u16>().unwrap_or(0);
    for _ in fraction.len()..3 {
        fraction_milli *= 10;
    }
    let quality = whole * 1000 + fraction_milli;
    if quality > 1000 {
        return Err(NegotiationError::invalid_header(
            HeaderField::Accept,
            format!("invalid q-value {value:?}"),
        ));
    }
    Ok(quality)
}

fn is_token(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
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
                )
        })
}
