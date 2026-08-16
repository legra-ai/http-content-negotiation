#![allow(missing_docs)]

use http_content_negotiation::{AcceptLanguage, LocalePolicy, SelectedLocale};

static SUPPORTED: &[&str] = &["en-US", "nl-NL"];

#[test]
fn quality_selects_the_preferred_locale() {
    let preferences = AcceptLanguage::parse("nl-NL;q=0.9, en-US;q=0.8").expect("valid header");

    assert_eq!(preferences.negotiate(SUPPORTED), Some("nl-NL"));
}

#[test]
fn primary_language_matches_a_supported_regional_locale() {
    let preferences = AcceptLanguage::parse("nl").expect("valid header");

    assert_eq!(preferences.negotiate(SUPPORTED), Some("nl-NL"));
}

#[test]
fn policy_uses_its_default_when_no_supported_language_matches() {
    let policy = LocalePolicy::new("en-US", SUPPORTED);

    assert_eq!(
        policy.negotiate(Some("fr-FR")).expect("valid header"),
        SelectedLocale::new("en-US")
    );
}
