# Official Fastly Rust exporter patch

Published 2026-10-11 · Updated 2026-10-11

The sample pins `fastly-opentelemetry-exporter = 0.2.0`, the official Fastly crate,
with its original package metadata and Fastly MIT copyright/license preserved in
`vendor/fastly-opentelemetry-exporter-0.2.0/`. Source:
[Fastly exporter repository](https://github.com/fastly/compute-rust-opentelemetry-exporter)
and [crate API](https://docs.rs/fastly-opentelemetry-exporter/0.2.0/fastly_opentelemetry_exporter/).

Upstream `src/span_exporter.rs::send_spans` calls `send_async(backend)`, drops the
pending request, and reports success when asynchronous initiation succeeds. It
does not wait for the HTTP response or check an ingest rejection. Provider
`force_flush` cannot make that transport acknowledgment reliable by itself.

This repository patches only that send path:

```diff
- let result = request.with_body(json).send_async(backend);
+ let result = request.with_body(json).send(backend);

- Ok(_) => OTelSdkResult::Ok(()),
+ Ok(response) if response.get_status().is_success() => OTelSdkResult::Ok(()),
+ Ok(response) => Err(OTelSdkError::InternalFailure(
+     format!("OTLP HTTP {}", response.get_status()))),
```

The service's `otel-http` backend bounds connection to 1 second, first byte to
3 seconds and interbyte delay to 1 second. The SDK provider uses the patched
exporter once, force-flushes, and shuts down before ending the request. There is
no duplicate second send. A thin SDK exporter wrapper reports failures on stderr
without logging request headers or secrets. SDK span collection/resource grouping and
`opentelemetry-proto` OTLP JSON encoding remain upstream code. The local exporter
uses those same official transforms and validates envelopes against a loopback
collector; no handwritten browser trace is substituted for RUM.

HTTP success remains an acknowledgment, not proof Corridora stored or correlated
the spans. Validate stored CLIENT → SERVER parentage separately. Review the patch
when upgrading the official crate, remove it when upstream waits/checks status,
and rerun native/Wasm builds plus real-RUM capture tests. Rustfmt changes formatting
without changing other upstream behavior.
