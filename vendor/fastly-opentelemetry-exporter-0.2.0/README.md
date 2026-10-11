# OpenTelemetry Exporter for Rust on Fastly Compute

An [OpenTelemetry](https://opentelemetry.io/) span exporter for [Fastly Compute](https://www.fastly.com/products/edge-compute) services.

This crate allows you to export traces from your Fastly Compute services to any OpenTelemetry-compatible backend using the OTLP HTTP protocol.

## Features

- Export OpenTelemetry traces via Fastly backends
- Automatic resource attributes from Fastly environment variables
- Trace context propagation from incoming requests
- Integration with the `tracing` ecosystem via `tracing-opentelemetry`

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
fastly-opentelemetry-exporter = "0.1.0"
```

## Usage

### Basic Setup

```rust
use fastly::{Backend, Error, Request, Response, http::StatusCode};
use fastly_opentelemetry_exporter::*;
use opentelemetry::trace::TracerProvider;
use opentelemetry_sdk::{
    propagation::TraceContextPropagator,
    trace::{Sampler, SdkTracerProvider},
};
use tracing::info;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::{Registry, layer::SubscriberExt};

#[fastly::main]
fn main(req: Request) -> Result<Response, Error> {
    // Set up the trace context propagator
    opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());

    // Create the exporter with your backend
    let backend = Backend::from_name("otel-http")?;
    let exporter = SpanExporterBuilder::new(backend)?.build()?;

    // Build the tracer provider
    let tracer_provider = SdkTracerProvider::builder()
        .with_resource(ResourceBuilder::build_default())
        .with_sampler(Sampler::AlwaysOn)
        .with_simple_exporter(exporter)
        .build();

    // Set up tracing subscriber with OpenTelemetry layer
    let subscriber = Registry::default().with(
        OpenTelemetryLayer::new(tracer_provider.tracer(env!("CARGO_PKG_NAME")))
            .with_tracked_inactivity(false)
            .with_threads(false),
    );
    tracing::subscriber::set_global_default(subscriber).unwrap();

    // Create the root span, extracting trace context from the incoming request
    let _guard = enter_root_span(&req);

    info!("Processing request");

    // Shutdown the tracer provider to flush spans
    drop(_guard);
    tracer_provider.shutdown().ok();

    Ok(Response::from_status(StatusCode::OK))
}
```

### Backend Configuration

You need to configure a backend in your Fastly service that points to your OpenTelemetry collector's HTTP endpoint. The exporter will automatically construct the URL using `https://<host>:<port>/v1/traces`.

You can override the URL if needed:

```rust
use fastly::http::Url;

let exporter = SpanExporterBuilder::new(backend)?
    .with_url(Url::parse("https://collector.example.com/v1/traces")?)
    .build()?;
```

### Resource Attributes

The `ResourceBuilder` automatically populates resource attributes from [Fastly Compute environment variables](https://www.fastly.com/documentation/reference/compute/ecp-env/):

| Attribute | Source |
|-----------|--------|
| `cloud.account.id` | `FASTLY_CUSTOMER_ID` |
| `cloud.availability_zone` | `FASTLY_POP` |
| `cloud.platform` | `"Fastly Compute"` |
| `cloud.provider` | `"Fastly"` |
| `cloud.region` | `FASTLY_REGION` |
| `deployment.environment.name` | `"staging"` or `"production"` based on `FASTLY_IS_STAGING` |
| `host.name` | `FASTLY_HOSTNAME` |
| `service.instance.id` | `FASTLY_SERVICE_ID` |
| `service.name` | Your crate's package name |
| `service.version` | `FASTLY_SERVICE_VERSION` |

You can customize the environment and service name:

```rust
let resource = ResourceBuilder::new()
    .with_environment("development")
    .with_service_name("my-service")
    .build();

let tracer_provider = SdkTracerProvider::builder()
    .with_resource(resource)
    // ...
    .build();
```

### Trace Context Propagation

The `enter_root_span` function extracts W3C Trace Context headers from incoming requests, allowing distributed traces to flow through your Fastly service:

```rust
// This will link the root span to any parent trace context
// from traceparent/tracestate headers
let _guard = enter_root_span(&req);
```

## Example

See the [example](./example/) directory for a complete working example.

To run the example:

1. Run `fastly compute publish` in the example directory to deploy the service
2. Create a backend named `otel-http` in the Fastly service that points to your OpenTelemetry HTTP collector endpoint
3. Make requests to your service and observe traces in your collector
