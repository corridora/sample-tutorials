use std::collections::HashMap;

use fastly::{
    Backend, Request,
    http::{HeaderName, HeaderValue, Method, Url},
};
use opentelemetry_proto::{
    tonic::collector::trace::v1::ExportTraceServiceRequest,
    transform::{
        common::tonic::ResourceAttributesWithSchema,
        trace::tonic::group_spans_by_resource_and_scope,
    },
};
use opentelemetry_sdk::{
    Resource,
    error::{OTelSdkError, OTelSdkResult},
    trace::SpanData,
};

use crate::{ExporterBuildError, SpanExporterBuilder};

/// An OpenTelemetry trace exporter
#[derive(Debug)]
pub struct SpanExporter {
    backend: Backend,
    headers: HashMap<HeaderName, Vec<HeaderValue>>,
    resource: ResourceAttributesWithSchema,
    url: Url,
}

impl SpanExporter {
    /// Create a new SpanExporter
    pub fn new(
        backend: Backend,
        resource: ResourceAttributesWithSchema,
        url: Url,
        headers: HashMap<HeaderName, Vec<HeaderValue>>,
    ) -> Self {
        Self {
            backend,
            headers,
            resource,
            url,
        }
    }

    /// Create a SpanExporterBuilder
    pub fn builder(backend: Backend) -> Result<SpanExporterBuilder, ExporterBuildError> {
        SpanExporterBuilder::new(backend)
    }
}

impl opentelemetry_sdk::trace::SpanExporter for SpanExporter {
    fn set_resource(&mut self, resource: &Resource) {
        self.resource = resource.into();
    }

    fn export(
        &self,
        batch: Vec<SpanData>,
    ) -> impl futures_util::Future<Output = OTelSdkResult> + std::marker::Send {
        let resource_spans = group_spans_by_resource_and_scope(batch, &self.resource);
        let req = ExportTraceServiceRequest { resource_spans };

        let json = match serde_json::to_string(&req) {
            Ok(json) => json,
            Err(e) => {
                return Box::pin(std::future::ready(OTelSdkResult::Err(
                    OTelSdkError::InternalFailure(e.to_string()),
                )));
            }
        };

        let backend = self.backend.clone();
        let headers = self.headers.clone();
        let url = self.url.clone();

        Box::pin(std::future::ready(send_spans(backend, url, headers, json)))
    }
}

fn send_spans(
    backend: Backend,
    url: Url,
    headers: HashMap<HeaderName, Vec<HeaderValue>>,
    json: String,
) -> OTelSdkResult {
    let mut request = Request::new(Method::POST, url);
    for (name, values) in headers.iter() {
        for value in values {
            request.append_header(name, value);
        }
    }

    let result = request.with_body(json).send(backend);

    match result {
        Ok(response) if response.get_status().is_success() => OTelSdkResult::Ok(()),
        Ok(response) => Err(OTelSdkError::InternalFailure(format!("OTLP HTTP {}", response.get_status()))),
        Err(e) => OTelSdkResult::Err(OTelSdkError::InternalFailure(e.to_string())),
    }
}
