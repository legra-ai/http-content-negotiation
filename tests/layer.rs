#![allow(missing_docs)]

use std::convert::Infallible;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::routing::get;
use bytes::Bytes;
use futures_util::stream;
use http_body_util::BodyExt;
use http_content_negotiation::{
    ContentNegotiationLayer, DeferredResponse, LocalePolicy, Representation, RepresentationId,
    RepresentationRegistry, RequestMediaTypes,
};
use tower::ServiceExt;

const JSON: Representation = Representation::new(RepresentationId::new("json"), "application/json");
const JSONL: Representation =
    Representation::new(RepresentationId::new("jsonl"), "application/x-ndjson");
static SUPPORTED_LOCALES: &[&str] = &["en-US", "nl-NL"];

#[tokio::test]
async fn layer_selects_jsonl_locale_and_streams_the_body() {
    let registry = RepresentationRegistry::new(JSON, [JSON, JSONL]);
    let layer = ContentNegotiationLayer::new(registry)
        .with_locale_policy(LocalePolicy::new("en-US", SUPPORTED_LOCALES));
    let app = Router::new()
        .route(
            "/",
            get(|| async {
                DeferredResponse::new(|context| {
                    assert_eq!(
                        context.representation().id(),
                        RepresentationId::new("jsonl")
                    );
                    assert_eq!(context.locale().expect("locale").as_str(), "nl-NL");
                    let body = stream::iter([
                        Ok::<Bytes, Infallible>(Bytes::from_static(b"{\"id\":1}\n")),
                        Ok(Bytes::from_static(b"{\"id\":2}\n")),
                    ]);
                    Ok(ResponseBuilder::build(
                        context.representation().media_type(),
                        Body::from_stream(body),
                    ))
                })
            }),
        )
        .layer(layer);

    let request = Request::builder()
        .uri("/")
        .header(header::ACCEPT, "application/x-ndjson")
        .header(header::ACCEPT_LANGUAGE, "nl-NL")
        .body(Body::empty())
        .expect("request");
    let response = app.oneshot(request).await.expect("response");
    let headers = response.headers();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "application/x-ndjson");
    assert_eq!(headers[header::VARY], "Accept, Accept-Language");
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    assert_eq!(&body[..], b"{\"id\":1}\n{\"id\":2}\n");
}

#[tokio::test]
async fn unmatchable_accept_rejects_before_the_handler_runs() {
    let registry = RepresentationRegistry::new(JSON, [JSON]);
    let called = Arc::new(AtomicBool::new(false));
    let called_by_handler = Arc::clone(&called);
    let app = Router::new()
        .route(
            "/",
            get(move || {
                let called = Arc::clone(&called_by_handler);
                async move {
                    called.store(true, Ordering::SeqCst);
                    "unexpected"
                }
            }),
        )
        .layer(ContentNegotiationLayer::new(registry));

    let request = Request::builder()
        .uri("/")
        .header(header::ACCEPT, "text/html")
        .body(Body::empty())
        .expect("request");
    let response = app.oneshot(request).await.expect("response");

    assert_eq!(response.status(), StatusCode::NOT_ACCEPTABLE);
    assert!(!called.load(Ordering::SeqCst));
}

#[tokio::test]
async fn request_content_type_can_be_validated_without_buffering_the_body() {
    let registry = RepresentationRegistry::new(JSON, [JSON]);
    let app = Router::new().route("/", get(|| async { "ok" })).layer(
        ContentNegotiationLayer::new(registry)
            .with_request_media_types(RequestMediaTypes::new(["application/json"])),
    );

    let request = Request::builder()
        .uri("/")
        .header(header::CONTENT_TYPE, "application/yaml")
        .body(Body::empty())
        .expect("request");
    let response = app.oneshot(request).await.expect("response");

    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

struct ResponseBuilder;

impl ResponseBuilder {
    fn build(media_type: &str, body: Body) -> axum::http::Response<Body> {
        axum::http::Response::builder()
            .header(header::CONTENT_TYPE, media_type)
            .body(body)
            .expect("test response must build")
    }
}
