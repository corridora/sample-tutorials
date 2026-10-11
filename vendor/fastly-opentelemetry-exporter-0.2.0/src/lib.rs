#![doc = include_str!("../README.md")]

mod error;
mod resource_builder;
mod root_span;
mod span_exporter;
mod span_exporter_builder;

pub use error::ExporterBuildError;
pub use resource_builder::ResourceBuilder;
pub use root_span::enter_root_span;
pub use span_exporter::SpanExporter;
pub use span_exporter_builder::SpanExporterBuilder;
