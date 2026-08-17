use axum::http::{HeaderMap, header};

use crate::error::{HeaderField, NegotiationError};

/// One language range from an `Accept-Language` header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocaleRange {
    tag: String,
    quality_milli: u16,
    order: usize,
}

impl LocaleRange {
    /// The normalized language range.
    pub fn tag(&self) -> &str {
        &self.tag
    }

    /// The quality value in thousandths.
    pub const fn quality_milli(&self) -> u16 {
        self.quality_milli
    }
}

/// Parsed `Accept-Language` preferences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptLanguage {
    // bounded: one HTTP header value is bounded by the server's header limit
    ranges: Vec<LocaleRange>,
}

impl AcceptLanguage {
    /// Parse an `Accept-Language` header.
    ///
    /// # Errors
    ///
    /// Returns [`NegotiationError::InvalidHeader`] for malformed language
    /// ranges or quality values.
    pub fn parse(header_value: &str) -> Result<Self, NegotiationError> {
        let mut ranges = Vec::new();
        for (order, member) in header_value.split(',').enumerate() {
            let mut parts = member.trim().split(';');
            let tag = parts.next().unwrap_or_default().trim();
            if tag.is_empty() {
                continue;
            }
            if tag != "*" && !is_language_range(tag) {
                return Err(NegotiationError::invalid_header(
                    HeaderField::AcceptLanguage,
                    format!("invalid language range {tag:?}"),
                ));
            }
            let mut quality_milli = 1000;
            for parameter in parts {
                let Some((key, value)) = parameter.trim().split_once('=') else {
                    continue;
                };
                if key.trim().eq_ignore_ascii_case("q") {
                    quality_milli = parse_quality_milli(value.trim())?;
                }
            }
            ranges.push(LocaleRange {
                tag: tag.to_ascii_lowercase(),
                quality_milli,
                order,
            });
        }
        ranges.sort_by(|left, right| {
            right
                .quality_milli
                .cmp(&left.quality_milli)
                .then_with(|| left.order.cmp(&right.order))
        });
        Ok(Self { ranges })
    }

    /// Whether no language preference was supplied.
    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    /// Negotiate against supported locale tags in server preference order.
    pub fn negotiate<'a>(&self, supported: &'a [&'static str]) -> Option<&'a str> {
        for range in &self.ranges {
            if range.quality_milli == 0 {
                continue;
            }
            if range.tag == "*" {
                return supported.first().copied();
            }
            if let Some(locale) = supported
                .iter()
                .find(|locale| locale.eq_ignore_ascii_case(&range.tag))
            {
                return Some(locale);
            }
            if let Some(primary) = range.tag.split('-').next()
                && let Some(locale) = supported.iter().find(|locale| {
                    locale
                        .split('-')
                        .next()
                        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(primary))
                })
            {
                return Some(locale);
            }
        }
        None
    }
}

/// The locale selected for the current request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedLocale(String);

impl SelectedLocale {
    /// Construct a selected locale tag.
    pub fn new(tag: impl Into<String>) -> Self {
        Self(tag.into())
    }

    /// Borrow the selected locale tag.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Locale negotiation policy for the Tower layer.
#[derive(Debug, Clone)]
pub struct LocalePolicy {
    default: &'static str,
    // bounded: application configuration contains a finite locale set
    supported: &'static [&'static str],
}

impl LocalePolicy {
    /// Construct a locale policy with a required default locale.
    pub const fn new(default: &'static str, supported: &'static [&'static str]) -> Self {
        Self { default, supported }
    }

    /// The configured default locale.
    pub const fn default_locale(&self) -> &'static str {
        self.default
    }

    /// The configured supported locales.
    pub const fn supported_locales(&self) -> &'static [&'static str] {
        self.supported
    }

    /// Select a locale from an optional request header.
    ///
    /// # Errors
    ///
    /// Returns [`NegotiationError::InvalidHeader`] when the header is
    /// malformed.
    pub fn negotiate(
        &self,
        header_value: Option<&str>,
    ) -> Result<SelectedLocale, NegotiationError> {
        let Some(header_value) = header_value else {
            return Ok(SelectedLocale::new(self.default));
        };
        let preferences = AcceptLanguage::parse(header_value)?;
        Ok(SelectedLocale::new(
            preferences
                .negotiate(self.supported)
                .unwrap_or(self.default),
        ))
    }
}

/// Extract `Accept-Language` from request headers.
///
/// # Errors
///
/// Returns [`NegotiationError::InvalidHeader`] when the header is not UTF-8.
pub fn accept_language_from_headers(headers: &HeaderMap) -> Result<Option<&str>, NegotiationError> {
    headers
        .get(header::ACCEPT_LANGUAGE)
        .map(|value| {
            value.to_str().map(Some).map_err(|_| {
                NegotiationError::invalid_header(HeaderField::AcceptLanguage, "value is not UTF-8")
            })
        })
        .transpose()
        .map(Option::flatten)
}

fn parse_quality_milli(value: &str) -> Result<u16, NegotiationError> {
    let value = value.trim_matches('"');
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    let whole = whole.parse::<u16>().map_err(|_| {
        NegotiationError::invalid_header(
            HeaderField::AcceptLanguage,
            format!("invalid q-value {value:?}"),
        )
    })?;
    if whole > 1 || fraction.len() > 3 || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(NegotiationError::invalid_header(
            HeaderField::AcceptLanguage,
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
            HeaderField::AcceptLanguage,
            format!("invalid q-value {value:?}"),
        ));
    }
    Ok(quality)
}

fn is_language_range(value: &str) -> bool {
    value.split('-').all(|part| {
        !part.is_empty() && part.len() <= 8 && part.bytes().all(|byte| byte.is_ascii_alphanumeric())
    })
}
