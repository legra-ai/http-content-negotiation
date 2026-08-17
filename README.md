# http-content-negotiation

[![Crates.io](https://img.shields.io/crates/v/http-content-negotiation.svg)](https://crates.io/crates/http-content-negotiation)
[![Documentation](https://docs.rs/http-content-negotiation/badge.svg)](https://docs.rs/http-content-negotiation)
[![CI](https://github.com/legra-ai/http-content-negotiation/actions/workflows/ci.yml/badge.svg)](https://github.com/legra-ai/http-content-negotiation/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/legra-ai/http-content-negotiation)

Axum and Tower content and language negotiation for streaming HTTP responses.

This crate settles representation metadata before a handler runs. It parses
`Accept`, `Content-Type`, and optionally `Accept-Language`, then exposes the
selection through typed request extensions. It does not serialize payloads,
collect bodies, or assume that JSON is the only representation.

## Why this crate

HTTP handlers should implement domain behavior, not repeat protocol policy:

- `Accept` parsing belongs in one tested layer;
- response formats can include JSON, JSONL, RDF streams, binary sequences, or
  application-specific media types;
- locale selection should be consistent across payloads, errors, and metadata;
- response encoders should receive a selected representation and produce an
  asynchronous body stream;
- a petabyte-scale payload must not become a `Vec`, `String`, or complete
  serialized document merely because it crossed an HTTP boundary.

The crate therefore separates representation selection from encoding. A
representation is an application-defined identifier plus a media type:

```rust
use http_content_negotiation::{
    ContentNegotiationLayer,
    Representation,
    RepresentationId,
    RepresentationRegistry,
    media_type,
};

const JSON: Representation = Representation::new(
    RepresentationId::new("json"),
    media_type::APPLICATION_JSON,
);
const JSONL: Representation = Representation::new(
    RepresentationId::new("jsonl"),
    media_type::APPLICATION_NDJSON,
);

let registry = RepresentationRegistry::new(JSON, [JSON, JSONL]);
let layer = ContentNegotiationLayer::new(registry);
```

`Accept: application/x-ndjson` selects `JSONL`; `Accept: application/json`
selects `JSON`. Quality values, wildcards, specificity, and `q=0` exclusions
are handled by the same parser.

## Handlers stay representation-agnostic

Handlers can return [`DeferredResponse`]. The handler does not parse request
headers or choose a format. The global layer invokes the renderer with the
selected representation and locale:

```rust,ignore
use axum::body::Body;
use axum::http::{Response, header};
use bytes::Bytes;
use futures_util::stream;
use http_content_negotiation::{DeferredResponse, RenderError};

async fn stream_records() -> DeferredResponse {
    DeferredResponse::new(|context| {
        let media_type = context.representation().media_type();
        let body = stream::iter([
            Ok::<Bytes, std::convert::Infallible>(Bytes::from_static(b"{\"id\":1}\n")),
            Ok(Bytes::from_static(b"{\"id\":2}\n")),
        ]);
        Response::builder()
            .header(header::CONTENT_TYPE, media_type)
            .body(Body::from_stream(body))
            .map_err(|error| RenderError::new(error.to_string()))
    })
}
```

The renderer must preserve streaming and backpressure. The negotiation layer
does not pre-scan, chunk, collect, or otherwise inspect the body.

## Locale negotiation

Locale selection is optional and policy-driven:

```rust
use http_content_negotiation::LocalePolicy;

static SUPPORTED: &[&str] = &["en-US", "nl-NL"];
let policy = LocalePolicy::new("en-US", SUPPORTED);
```

The selected locale is available as a [`SelectedLocale`](https://docs.rs/http-content-negotiation/latest/http_content_negotiation/struct.SelectedLocale.html)
request extension and to deferred renderers through [`RenderContext`]. The
layer also adds the appropriate `Vary` headers.

This crate does not contain application translation catalogs. Applications
decide how language-tagged payload values, error messages, and metadata are
translated.

## Scope

This crate provides:

- strict q-value-aware `Accept` parsing;
- media-type wildcard and specificity matching;
- registered response representation selection;
- request `Content-Type` extraction and optional validation;
- optional `Accept-Language` negotiation;
- Axum/Tower middleware and deferred streaming response rendering;
- `Vary` and selected `Content-Type` response metadata.

This crate deliberately does not provide:

- JSON, JSONL, YAML, RDF, or binary serializers;
- a universal response envelope;
- translation catalogs or application error contracts;
- whole-body request decoding;
- a compatibility fallback that silently changes the selected format.

Streaming envelope framing and format-specific encoders can be layered on top
of the selected representation.

## License

Licensed under either of:

- MIT license ([`LICENSE-MIT`](LICENSE-MIT) or <https://opensource.org/licenses/MIT>);
- Apache License, Version 2.0 ([`LICENSE-APACHE`](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>).

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in this crate by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.
