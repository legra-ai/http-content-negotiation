use axum::http::{HeaderMap, header};
use unic_langid::LanguageIdentifier;

use crate::error::{HeaderField, NegotiationError};

/// One language range from an `Accept-Language` header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocaleRange {
    range: LanguageRange,
    quality_milli: u16,
    order: usize,
}

impl LocaleRange {
    /// The typed language range.
    pub const fn range(&self) -> &LanguageRange {
        &self.range
    }

    /// The quality value in thousandths.
    pub const fn quality_milli(&self) -> u16 {
        self.quality_milli
    }
}

/// A typed language range from `Accept-Language`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LanguageRange {
    /// Match the first supported locale.
    Any,
    /// Match a parsed BCP 47 language identifier.
    Language(LanguageIdentifier),
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
            let range = if tag == "*" {
                LanguageRange::Any
            } else {
                LanguageRange::Language(tag.parse().map_err(|error| {
                    NegotiationError::invalid_header(
                        HeaderField::AcceptLanguage,
                        format!("invalid language range {tag:?}: {error}"),
                    )
                })?)
            };
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
                range,
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

    /// The parsed language ranges ordered by quality and header order.
    pub fn ranges(&self) -> &[LocaleRange] {
        &self.ranges
    }

    /// Negotiate against supported locales in server preference order.
    pub fn negotiate<'a>(
        &self,
        supported: &'a [LanguageIdentifier],
    ) -> Option<&'a LanguageIdentifier> {
        for range in &self.ranges {
            if range.quality_milli == 0 {
                continue;
            }
            if matches!(range.range, LanguageRange::Any) {
                return supported.first();
            }
            let LanguageRange::Language(language) = &range.range else {
                unreachable!("all language ranges are matched above");
            };
            if let Some(locale) = supported.iter().find(|locale| *locale == language) {
                return Some(locale);
            }
            if let Some(locale) = supported
                .iter()
                .find(|locale| locale.language == language.language)
            {
                return Some(locale);
            }
        }
        None
    }
}

/// The locale selected for the current request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedLocale(LanguageIdentifier);

impl SelectedLocale {
    /// Construct a selected locale from a validated BCP 47 identifier.
    pub const fn new(locale: LanguageIdentifier) -> Self {
        Self(locale)
    }

    /// Borrow the selected BCP 47 language identifier.
    pub const fn as_langid(&self) -> &LanguageIdentifier {
        &self.0
    }

    /// Consume the selected locale and return its language identifier.
    pub fn into_langid(self) -> LanguageIdentifier {
        self.0
    }
}

/// Locale negotiation policy for the Tower layer.
#[derive(Debug, Clone)]
pub struct LocalePolicy {
    default: LanguageIdentifier,
    // bounded: application configuration contains a finite locale set
    supported: &'static [LanguageIdentifier],
}

impl LocalePolicy {
    /// Construct a locale policy with a required default locale.
    pub const fn new(
        default: LanguageIdentifier,
        supported: &'static [LanguageIdentifier],
    ) -> Self {
        Self { default, supported }
    }

    /// The configured default locale.
    pub const fn default_locale(&self) -> &LanguageIdentifier {
        &self.default
    }

    /// The configured supported locales.
    pub const fn supported_locales(&self) -> &'static [LanguageIdentifier] {
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
            return Ok(SelectedLocale::new(self.default.clone()));
        };
        let preferences = AcceptLanguage::parse(header_value)?;
        Ok(SelectedLocale::new(
            preferences
                .negotiate(self.supported)
                .unwrap_or(&self.default)
                .clone(),
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
