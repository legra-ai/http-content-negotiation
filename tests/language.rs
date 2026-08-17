#![allow(missing_docs)]

use http_content_negotiation::{
    AcceptLanguage, LanguageIdentifier, LanguageRange, LocalePolicy, SelectedLocale,
};

static SUPPORTED: &[LanguageIdentifier] = unic_langid::langid_slice!["en-US", "nl-NL"];

#[test]
fn quality_selects_the_preferred_locale() {
    let preferences = AcceptLanguage::parse("nl-NL;q=0.9, en-US;q=0.8").expect("valid header");

    assert_eq!(preferences.negotiate(SUPPORTED), Some(&SUPPORTED[1]));
}

#[test]
fn primary_language_matches_a_supported_regional_locale() {
    let preferences = AcceptLanguage::parse("nl").expect("valid header");

    assert_eq!(preferences.negotiate(SUPPORTED), Some(&SUPPORTED[1]));
}

#[test]
fn policy_uses_its_default_when_no_supported_language_matches() {
    let policy = LocalePolicy::new(SUPPORTED[0].clone(), SUPPORTED);

    assert_eq!(
        policy.negotiate(Some("fr-FR")).expect("valid header"),
        SelectedLocale::new(SUPPORTED[0].clone())
    );
}

#[test]
fn wildcard_is_a_typed_language_range() {
    let preferences = AcceptLanguage::parse("*;q=0.5").expect("valid header");

    assert_eq!(preferences.ranges()[0].range(), &LanguageRange::Any);
}

#[test]
fn malformed_language_identifier_is_rejected() {
    let error = AcceptLanguage::parse("en-@").expect_err("invalid subtag is not BCP 47");

    assert!(error.to_string().contains("invalid language range"));
}
