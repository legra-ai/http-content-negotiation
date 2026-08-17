use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use axum::body::Body;
use axum::http::{Request, Response, StatusCode, header};
use axum::response::IntoResponse;
use tower::{Layer, Service};

use crate::error::{HeaderField, NegotiationError};
use crate::language::{LocalePolicy, SelectedLocale, accept_language_from_headers};
use crate::media::{
    NegotiatedRepresentation, RepresentationRegistry, RequestMediaType, RequestMediaTypes,
};
use crate::media_type::TEXT_PLAIN_UTF8;

/// Context passed to a deferred streaming response renderer.
#[derive(Debug, Clone)]
pub struct RenderContext {
    representation: NegotiatedRepresentation,
    request_media_type: Option<RequestMediaType>,
    locale: Option<SelectedLocale>,
}

impl RenderContext {
    pub(crate) const fn new(
        representation: NegotiatedRepresentation,
        request_media_type: Option<RequestMediaType>,
        locale: Option<SelectedLocale>,
    ) -> Self {
        Self {
            representation,
            request_media_type,
            locale,
        }
    }

    /// The selected response representation.
    pub const fn representation(&self) -> NegotiatedRepresentation {
        self.representation
    }

    /// The request media type, when the request supplied `Content-Type`.
    pub fn request_media_type(&self) -> Option<&RequestMediaType> {
        self.request_media_type.as_ref()
    }

    /// The selected response locale, when locale negotiation is enabled.
    pub fn locale(&self) -> Option<&SelectedLocale> {
        self.locale.as_ref()
    }
}

/// A streaming response rendering failure.
#[derive(Debug, Clone, thiserror::Error)]
#[error("streaming response rendering failed: {message}")]
pub struct RenderError {
    message: String,
}

impl RenderError {
    /// Construct a rendering error from an application or encoder message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

type Renderer = Box<dyn FnOnce(RenderContext) -> Result<Response<Body>, RenderError> + Send>;

/// A response whose streaming encoder is selected by the global Tower layer.
///
/// The renderer is invoked exactly once after request negotiation. It should
/// return a response whose body is an asynchronous stream; this type never
/// collects or buffers that body.
#[derive(Clone)]
pub struct DeferredResponse {
    renderer: Arc<Mutex<Option<Renderer>>>,
}

impl DeferredResponse {
    /// Construct a deferred response renderer.
    pub fn new<F>(renderer: F) -> Self
    where
        F: FnOnce(RenderContext) -> Result<Response<Body>, RenderError> + Send + 'static,
    {
        Self {
            renderer: Arc::new(Mutex::new(Some(Box::new(renderer)))),
        }
    }

    fn render(self, context: RenderContext) -> Result<Response<Body>, RenderError> {
        let renderer = self
            .renderer
            .lock()
            .expect("deferred response renderer mutex poisoned")
            .take()
            .expect("deferred response renderer already consumed");
        renderer(context)
    }
}

impl IntoResponse for DeferredResponse {
    fn into_response(self) -> Response<Body> {
        let mut response = Response::new(Body::empty());
        response.extensions_mut().insert(self);
        response
    }
}

/// Tower layer that negotiates response representations, request media types,
/// and optional locales before invoking handlers.
#[derive(Clone)]
pub struct ContentNegotiationLayer {
    registry: Arc<RepresentationRegistry>,
    locale_policy: Option<LocalePolicy>,
    request_media_types: Option<RequestMediaTypes>,
    error_renderer: Arc<dyn Fn(&NegotiationError) -> Response<Body> + Send + Sync>,
}

impl ContentNegotiationLayer {
    /// Construct a layer with the response representation registry.
    pub fn new(registry: RepresentationRegistry) -> Self {
        Self {
            registry: Arc::new(registry),
            locale_policy: None,
            request_media_types: None,
            error_renderer: Arc::new(default_error_response),
        }
    }

    /// Set the response renderer for negotiation failures.
    ///
    /// The default renderer is a plain-text response. Applications with a
    /// structured error contract can replace it without wrapping or reading
    /// the response body, preserving the layer's streaming boundary.
    pub fn with_error_renderer<F>(mut self, renderer: F) -> Self
    where
        F: Fn(&NegotiationError) -> Response<Body> + Send + Sync + 'static,
    {
        self.error_renderer = Arc::new(renderer);
        self
    }

    /// Add `Accept-Language` negotiation.
    pub fn with_locale_policy(mut self, policy: LocalePolicy) -> Self {
        self.locale_policy = Some(policy);
        self
    }

    /// Validate request `Content-Type` against an application-defined set.
    pub fn with_request_media_types(mut self, media_types: RequestMediaTypes) -> Self {
        self.request_media_types = Some(media_types);
        self
    }
}

impl<S> Layer<S> for ContentNegotiationLayer {
    type Service = ContentNegotiationService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ContentNegotiationService {
            inner,
            registry: Arc::clone(&self.registry),
            locale_policy: self.locale_policy.clone(),
            request_media_types: self.request_media_types.clone(),
            error_renderer: Arc::clone(&self.error_renderer),
        }
    }
}

/// The service produced by [`ContentNegotiationLayer`].
#[derive(Clone)]
pub struct ContentNegotiationService<S> {
    inner: S,
    registry: Arc<RepresentationRegistry>,
    locale_policy: Option<LocalePolicy>,
    request_media_types: Option<RequestMediaTypes>,
    error_renderer: Arc<dyn Fn(&NegotiationError) -> Response<Body> + Send + Sync>,
}

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

impl<S> Service<Request<Body>> for ContentNegotiationService<S>
where
    S: Service<Request<Body>, Response = Response<Body>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = Response<Body>;
    type Error = S::Error;
    type Future = BoxFuture<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut request: Request<Body>) -> Self::Future {
        let negotiation = negotiate_request(
            request.headers(),
            &self.registry,
            self.locale_policy.as_ref(),
            self.request_media_types.as_ref(),
        );
        let mut inner = self.inner.clone();
        let error_renderer = Arc::clone(&self.error_renderer);

        Box::pin(async move {
            let (representation, request_media_type, locale, locale_enabled) = match negotiation {
                Ok(result) => result,
                Err(error) => return Ok((error_renderer)(&error)),
            };

            request.extensions_mut().insert(representation);
            if let Some(request_media_type) = request_media_type.clone() {
                request.extensions_mut().insert(request_media_type);
            }
            if let Some(locale) = locale.clone() {
                request.extensions_mut().insert(locale);
            }

            let response = inner.call(request).await?;
            Ok(finish_response(
                response,
                representation,
                request_media_type,
                locale,
                locale_enabled,
            ))
        })
    }
}

fn negotiate_request(
    headers: &axum::http::HeaderMap,
    registry: &RepresentationRegistry,
    locale_policy: Option<&LocalePolicy>,
    request_media_types: Option<&RequestMediaTypes>,
) -> Result<
    (
        NegotiatedRepresentation,
        Option<RequestMediaType>,
        Option<SelectedLocale>,
        bool,
    ),
    NegotiationError,
> {
    let accept = header_value(headers, header::ACCEPT, HeaderField::Accept)?;
    let representation = registry.negotiate(accept)?;
    let request_media_type = RequestMediaType::from_headers(headers)?;
    let request_media_type = match request_media_types {
        Some(types) => types.validate(request_media_type)?,
        None => request_media_type,
    };
    let locale = match locale_policy {
        Some(policy) => Some(policy.negotiate(accept_language_from_headers(headers)?)?),
        None => None,
    };
    Ok((
        representation,
        request_media_type,
        locale,
        locale_policy.is_some(),
    ))
}

fn finish_response(
    response: Response<Body>,
    representation: NegotiatedRepresentation,
    request_media_type: Option<RequestMediaType>,
    locale: Option<SelectedLocale>,
    locale_enabled: bool,
) -> Response<Body> {
    // This function is split out so the response metadata path remains
    // independent of any encoder implementation.
    let mut response = response;
    let deferred = response.extensions_mut().remove::<DeferredResponse>();
    if let Some(deferred) = deferred {
        let context = RenderContext::new(representation, request_media_type, locale);
        match deferred.render(context) {
            Ok(rendered) => response = rendered,
            Err(error) => response = render_error_response(&error),
        }
    }
    add_response_headers(&mut response, representation, locale_enabled);
    response
}

fn add_response_headers(
    response: &mut Response<Body>,
    representation: NegotiatedRepresentation,
    locale_enabled: bool,
) {
    if response.status().is_success() && !response.headers().contains_key(header::CONTENT_TYPE) {
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static(representation.media_type()),
        );
    }
    append_vary(response, "Accept");
    if locale_enabled {
        append_vary(response, "Accept-Language");
    }
}

fn append_vary(response: &mut Response<Body>, name: &str) {
    if response
        .headers()
        .get(header::VARY)
        .is_some_and(|value| value.as_bytes() == b"*")
    {
        return;
    }
    let existing = response
        .headers()
        .get(header::VARY)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if existing
        .split(',')
        .any(|value| value.trim().eq_ignore_ascii_case(name))
    {
        return;
    }
    let value = if existing.is_empty() {
        name.to_owned()
    } else {
        format!("{existing}, {name}")
    };
    response.headers_mut().insert(
        header::VARY,
        header::HeaderValue::from_str(&value).expect("Vary value must be valid ASCII"),
    );
}

fn header_value(
    headers: &axum::http::HeaderMap,
    name: axum::http::HeaderName,
    field: HeaderField,
) -> Result<Option<&str>, NegotiationError> {
    headers
        .get(name)
        .map(|value| {
            value
                .to_str()
                .map(Some)
                .map_err(|_| NegotiationError::invalid_header(field, "value is not UTF-8"))
        })
        .transpose()
        .map(Option::flatten)
}

fn default_error_response(error: &NegotiationError) -> Response<Body> {
    let status = error.status();
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, TEXT_PLAIN_UTF8)
        .body(Body::from(error.to_string()))
        .expect("static negotiation error response must build")
}

fn render_error_response(error: &RenderError) -> Response<Body> {
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .header(header::CONTENT_TYPE, TEXT_PLAIN_UTF8)
        .body(Body::from(error.to_string()))
        .expect("static render error response must build")
}
