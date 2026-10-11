use fastly::Request;
use opentelemetry::{propagation::Extractor, trace::TraceContextExt};
use tracing::{Span, info_span, span::EnteredSpan};
use tracing_opentelemetry::{OpenTelemetrySpanExt, SetParentError};

/// Create and enter a root span, extracting trace context from the incoming request.
///
/// This function creates an info-level span named "root" and extracts W3C Trace Context
/// headers (traceparent/tracestate) from the incoming request, allowing distributed traces
/// to flow through your Fastly service.
///
/// The returned [`EnteredSpan`] guard must be held for the duration of the request
/// processing. When dropped, the span will be closed and exported.
///
/// # Example
///
/// ```no_run
/// use fastly::{Request, http::Method};
/// use fastly_opentelemetry_exporter::enter_root_span;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let req = Request::new(Method::GET, "/");
/// let _guard = enter_root_span(&req)?;
/// // Process request...
/// drop(_guard); // Explicitly close the span
/// # Ok(())
/// # }
/// ```
pub fn enter_root_span(req: &Request) -> Result<EnteredSpan, SetParentError> {
    let span = info_span!("root");

    update_span_for_request(&span, req)?;

    Ok(span.entered())
}

struct HeaderExtractor<'a>(pub &'a Request);

impl Extractor for HeaderExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get_header(key).and_then(|v| v.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.get_header_names().map(|n| n.as_str()).collect()
    }
}

/// Update a span with trace context extracted from a request.
///
/// This function extracts W3C Trace Context headers from the request and sets the
/// span's parent context accordingly. Returns the trace ID as a string.
///
/// # Errors
///
/// Returns a [`SetParentError`] if setting the parent context fails.
pub fn update_span_for_request(span: &Span, req: &Request) -> Result<String, SetParentError> {
    let cx = opentelemetry::global::get_text_map_propagator(|propagator| {
        propagator.extract(&HeaderExtractor(req))
    });

    span.set_parent(cx)?;
    Ok(span.context().span().span_context().trace_id().to_string())
}
