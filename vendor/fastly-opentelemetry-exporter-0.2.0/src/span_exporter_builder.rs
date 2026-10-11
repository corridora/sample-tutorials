use std::{collections::HashMap, str::FromStr};

use fastly::{
    Backend,
    convert::{ToHeaderName, ToHeaderValue},
    http::{HeaderName, HeaderValue, Url, header::CONTENT_TYPE},
};
use opentelemetry_proto::transform::common::tonic::ResourceAttributesWithSchema;

use crate::{ExporterBuildError, SpanExporter};

/// Build a [`SpanExporter`] that exports OTEL spans to a [`Backend`]
///
/// You can provide an [OTEL resource][ResourceAttributesWithSchema], any headers necessary for the
/// target collector endpoint, and override the URL.
///
/// To create a `SpanExporter` that uses the "OTEL" backend using its host and port with the
/// default path `/v1/traces' and supply a bearer token for authorization:
///
/// ```no_run
/// use fastly::Backend;
/// use fastly_opentelemetry_exporter::SpanExporter;
///
/// let span_exporter = SpanExporter::builder(Backend::from_name("OTEL")?)?
///     .with_header("Authorization", "Bearer TOKEN_HERE")
///     .build()?;
/// ```
pub struct SpanExporterBuilder {
    backend: Backend,
    headers: HashMap<HeaderName, Vec<HeaderValue>>,
    resource: Option<ResourceAttributesWithSchema>,
    url: Option<Url>,
}

impl SpanExporterBuilder {
    /// Create a new SpanExporterBuilder that will export spans to a [`fastly::Backend`] with default settings
    pub fn new(backend: Backend) -> Result<Self, ExporterBuildError> {
        if !backend.exists() {
            return Err(ExporterBuildError::MissingBackend {
                name: backend.name().into(),
            });
        }

        let mut headers: HashMap<HeaderName, Vec<HeaderValue>> = Default::default();

        headers
            .entry(CONTENT_TYPE)
            .or_default()
            .push(HeaderValue::from_static("application/json"));

        Ok(Self {
            backend,
            headers,
            resource: None,
            url: None,
        })
    }

    /// Build the span exporter
    pub fn build(self) -> Result<SpanExporter, ExporterBuildError> {
        let backend = self.backend;

        let resource = self.resource.unwrap_or_default();

        let url = self.url.map_or_else(|| default_url(&backend), Ok)?;

        Ok(SpanExporter::new(backend, resource, url, self.headers))
    }

    /// Remove all values for a header from the SpanExporter
    pub fn remove_header(mut self, name: impl ToHeaderName) -> Self {
        self.headers.remove(&name.into_owned());

        self
    }

    /// Add a header to each request made by the SpanExporter
    ///
    /// Headers with duplicate names will be appended
    pub fn with_header(mut self, name: impl ToHeaderName, value: impl ToHeaderValue) -> Self {
        let entry = self.headers.entry(name.into_owned()).or_default();

        entry.push(value.into_owned());

        self
    }

    /// Add a set of headers to each request made by the SpanExporter
    ///
    /// Headers with duplicate names will be appended
    pub fn with_headers<Name, Value>(mut self, headers: HashMap<Name, Vec<Value>>) -> Self
    where
        Name: ToHeaderName,
        Value: ToHeaderValue,
    {
        for (name, values) in headers.into_iter() {
            let entry = self.headers.entry(name.into_owned()).or_default();

            for value in values {
                entry.push(value.into_owned());
            }
        }

        self
    }

    /// Override the exporter resource
    pub fn with_resource(self, resource: ResourceAttributesWithSchema) -> Self {
        Self {
            resource: Some(resource),
            ..self
        }
    }

    /// Override the exporter URL
    ///
    /// If the URL is left unspecified it will be constructed from the backend and export traces to
    /// `/v1/traces`
    pub fn with_url(self, url: Url) -> Self {
        Self {
            url: Some(url),
            ..self
        }
    }
}

fn default_url(backend: &Backend) -> Result<Url, ExporterBuildError> {
    let mut url = Url::from_str(&format!(
        "https://{}:{}/v1/traces",
        backend.get_host(),
        backend.get_port()
    ))
    .unwrap();

    if !backend.is_ssl() {
        url.set_scheme("http").unwrap()
    }

    Ok(url)
}
