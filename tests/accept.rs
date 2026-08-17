#![allow(missing_docs)]

use http_content_negotiation::{
    HeaderField, NegotiationError, ParsedAccept, Representation, RepresentationId, media_type,
};

const JSON: Representation =
    Representation::new(RepresentationId::new("json"), media_type::APPLICATION_JSON);
const JSONL: Representation = Representation::new(
    RepresentationId::new("jsonl"),
    media_type::APPLICATION_NDJSON,
);

#[test]
fn quality_and_specificity_select_the_best_representation() {
    let parsed = ParsedAccept::parse(
        "application/json;q=0.4, application/*;q=0.6, application/x-ndjson;q=0.9",
    )
    .expect("valid Accept header");

    assert_eq!(parsed.negotiate(&[JSON, JSONL]), Some(&JSONL));
}

#[test]
fn wildcard_ties_use_server_candidate_order() {
    let parsed = ParsedAccept::parse("*/*").expect("valid Accept header");

    assert_eq!(parsed.negotiate(&[JSON, JSONL]), Some(&JSON));
}

#[test]
fn zero_quality_excludes_a_candidate() {
    let parsed = ParsedAccept::parse("*/*, application/json;q=0").expect("valid Accept header");

    assert_eq!(parsed.negotiate(&[JSON, JSONL]), Some(&JSONL));
}

#[test]
fn malformed_quality_fails_fast() {
    let error = ParsedAccept::parse("application/json;q=2").expect_err("invalid q-value");

    assert!(matches!(
        error,
        NegotiationError::InvalidHeader {
            name: HeaderField::Accept,
            ..
        }
    ));
}

#[test]
fn unmatchable_accept_is_not_acceptable() {
    let error = ParsedAccept::parse("text/html")
        .expect("valid header")
        .negotiate(&[JSON])
        .is_none();

    assert!(error);
}
