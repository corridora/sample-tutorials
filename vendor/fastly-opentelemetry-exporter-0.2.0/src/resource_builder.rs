use opentelemetry::KeyValue;
use opentelemetry_sdk::Resource;
use opentelemetry_semantic_conventions::{
    SCHEMA_URL,
    resource::{
        CLOUD_ACCOUNT_ID, CLOUD_AVAILABILITY_ZONE, CLOUD_PLATFORM, CLOUD_PROVIDER, CLOUD_REGION,
        DEPLOYMENT_ENVIRONMENT_NAME, HOST_NAME, SERVICE_INSTANCE_ID, SERVICE_NAME, SERVICE_VERSION,
    },
};

/// Build a default trace [`Resource`] containing attributes from [Fastly compute environment
/// variables] and package settings.
///
/// If `environment` is not overridden the value is derived from `FASTLY_IS_STAGING`
///
/// If `service_name` is not overriden the value is the crate package name.
///
/// [Fastly compute environment variables]: https://www.fastly.com/documentation/reference/compute/ecp-env/
#[derive(Default)]
pub struct ResourceBuilder {
    environment: Option<String>,
    service_name: Option<String>,
}

impl ResourceBuilder {
    /// Create a `ResourceBuilder`
    pub fn new() -> Self {
        Default::default()
    }

    /// Build a `Resource`
    pub fn build(self) -> Resource {
        let environment = self.environment.unwrap_or_else(|| {
            let is_staging = std::env::var("FASTLY_IS_STAGING").unwrap_or("0".to_string());

            if is_staging == "1" {
                "staging"
            } else {
                "production"
            }
            .to_string()
        });

        let service_name = self
            .service_name
            .unwrap_or_else(|| env!("CARGO_PKG_NAME").to_string());

        Resource::builder()
            .with_service_name(service_name.clone())
            .with_schema_url(
                [
                    KeyValue::new(CLOUD_ACCOUNT_ID, value_from_env("FASTLY_CUSTOMER_ID")),
                    KeyValue::new(CLOUD_AVAILABILITY_ZONE, value_from_env("FASTLY_POP")),
                    KeyValue::new(CLOUD_PLATFORM, "Fastly Compute"),
                    KeyValue::new(CLOUD_PROVIDER, "Fastly"),
                    KeyValue::new(CLOUD_REGION, value_from_env("FASTLY_REGION")),
                    KeyValue::new(DEPLOYMENT_ENVIRONMENT_NAME, environment),
                    KeyValue::new(HOST_NAME, value_from_env("FASTLY_HOSTNAME")),
                    KeyValue::new(SERVICE_INSTANCE_ID, value_from_env("FASTLY_SERVICE_ID")),
                    KeyValue::new(SERVICE_NAME, service_name),
                    KeyValue::new(SERVICE_VERSION, value_from_env("FASTLY_SERVICE_VERSION")),
                ],
                SCHEMA_URL,
            )
            .build()
    }

    /// Build a resource using the defaults
    pub fn build_default() -> Resource {
        Self::default().build()
    }

    /// Override the deployment environment name
    pub fn with_environment(self, environment: &str) -> Self {
        Self {
            environment: Some(environment.to_string()),
            ..self
        }
    }

    /// Override the service name
    pub fn with_service_name(self, service_name: &str) -> Self {
        Self {
            service_name: Some(service_name.to_string()),
            ..self
        }
    }
}

fn value_from_env(fastly_customer_id: &str) -> String {
    std::env::var(fastly_customer_id).unwrap_or("unknown".to_string())
}
