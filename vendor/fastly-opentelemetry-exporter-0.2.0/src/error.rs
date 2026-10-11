/// Errors that can occur when building a span exporter.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ExporterBuildError {
    /// The provided URL was invalid.
    #[error("Invalid URL: {message} ({url:?})")]
    InvalidUrl {
        /// The URL that failed to parse
        url: String,
        /// The error message
        message: String,
    },
    /// The specified backend does not exist in the Fastly service configuration.
    #[error("Exporter backend {name} does not exist")]
    MissingBackend {
        /// The name of the missing backend
        name: String,
    },
}
