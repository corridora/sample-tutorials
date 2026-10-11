use fastly::{
    Backend, Error, Request, Response, config_store::ConfigStore, kv_store::KVStore,
    secret_store::SecretStore,
};
use fastly_opentelemetry_exporter::SpanExporterBuilder;
use opentelemetry_sdk::trace::SdkTracerProvider;
use serde_json::{Value, json};
use std::io::{Read, Write};
use tutorials::{Headers, Runtime};
#[derive(Debug)]
struct CheckedExporter(fastly_opentelemetry_exporter::SpanExporter);
impl opentelemetry_sdk::trace::SpanExporter for CheckedExporter {
    fn set_resource(&mut self, resource: &opentelemetry_sdk::Resource) {
        opentelemetry_sdk::trace::SpanExporter::set_resource(&mut self.0, resource);
    }
    fn export(
        &self,
        batch: Vec<opentelemetry_sdk::trace::SpanData>,
    ) -> impl std::future::Future<Output = opentelemetry_sdk::error::OTelSdkResult> + Send {
        async move {
            let result = opentelemetry_sdk::trace::SpanExporter::export(&self.0, batch).await;
            if result.is_err() {
                eprintln!("OTLP traces failed: bounded transport or non-2xx ingestion response");
            }
            result
        }
    }
}
struct Edge;
impl Runtime for Edge {
    fn config(&self, k: &str) -> String {
        if ["FASTLY_POP", "FASTLY_REGION"].contains(&k) {
            return std::env::var(k).unwrap_or_default();
        }
        if k == "SERVICE_VERSION" {
            return option_env!("SAMPLE_BUILD_VERSION")
                .unwrap_or(env!("CARGO_PKG_VERSION"))
                .into();
        }
        if [
            "CORRIDORA_INGEST_KEY",
            "DEMO_CONTROL_TOKEN",
            "DOWNSTREAM_AUTH_TOKEN",
        ]
        .contains(&k)
        {
            return SecretStore::open("tutorial-secrets")
                .ok()
                .and_then(|s| s.get(k))
                .and_then(|s| String::from_utf8(s.plaintext().to_vec()).ok())
                .unwrap_or_default();
        }
        ConfigStore::open("tutorial-config")
            .try_get(k)
            .ok()
            .flatten()
            .unwrap_or_default()
    }
    fn read(&mut self, k: &str) -> Option<String> {
        KVStore::open("tutorial-progress")
            .ok()
            .flatten()?
            .lookup(k)
            .ok()
            .map(|mut v| v.take_body().into_string())
    }
    fn write(&mut self, k: &str, v: &str) -> Result<(), String> {
        KVStore::open("tutorial-progress")
            .map_err(|_| "KV unavailable")?
            .ok_or("KV missing")?
            .insert(k, v)
            .map_err(|_| "KV write failed".into())
    }
    fn origin(&mut self, u: &str, h: Headers, pass: bool) -> Result<(Value, String), String> {
        let url = if u.starts_with("local://") {
            if pass {
                "https://fixture.invalid/api/fixture?slow=1"
            } else {
                "https://fixture.invalid/api/fixture"
            }
        } else {
            u
        };
        let mut req = Request::get(url);
        for (k, v) in h {
            req.set_header(k, v);
        }
        req.set_pass(pass);
        let backend = if u.starts_with("local://") {
            "fixture-origin"
        } else {
            "portfolio-origin"
        };
        let mut res = req.send(backend).map_err(|_| "origin request failed")?;
        let state = res
            .get_header_str("x-cache")
            .unwrap_or(if pass { "PASS" } else { "UNKNOWN" })
            .to_owned();
        let body = res.take_body().into_string();
        let value = serde_json::from_str(&body).map_err(|_| "origin JSON invalid")?;
        Ok((value, state))
    }
    fn send_signal(&mut self, s: &str, v: Value) -> Result<(), String> {
        let key = self.config("CORRIDORA_INGEST_KEY");
        if key.is_empty() {
            return Ok(());
        }
        let base = self.config("CORRIDORA_OTLP_ENDPOINT");
        let res = Request::post(format!("{}/v1/{s}", base.trim_end_matches('/')))
            .with_header("Content-Type", "application/json")
            .with_header("X-CORRIDORA-KEY", key)
            .with_body(v.to_string())
            .send("otel-http")
            .map_err(|_| format!("{s} export failed"))?;
        if !res.get_status().is_success() {
            return Err(format!("{s} ingest rejected"));
        }
        Ok(())
    }
    fn static_file(&self, p: &str) -> Option<(Vec<u8>, String)> {
        if p == "/runtime-config.js" {
            return Some((format!("window.SAMPLE_CONFIG={};",json!({"rumKey":self.config("CORRIDORA_RUM_KEY"),"rumScript":self.config("CORRIDORA_RUM_SCRIPT_URL"),"endpoint":self.config("CORRIDORA_OTLP_ENDPOINT"),"namespace":self.config("OTEL_SERVICE_NAMESPACE"),"environment":self.config("DEPLOYMENT_ENVIRONMENT"),"version":self.config("SERVICE_VERSION"),"trackerUrl":self.config("TRACKER_SIMULATOR_URL")})).into_bytes(),"application/javascript".into()));
        }
        assets::get(p)
    }
}
mod assets {
    include!(concat!(env!("OUT_DIR"), "/assets.rs"));
}
#[fastly::main]
fn main(mut req: Request) -> Result<Response, Error> {
    let start = std::time::SystemTime::now();
    let mut rt = Edge;
    let base = rt.config("CORRIDORA_OTLP_ENDPOINT");
    let exporter = SpanExporterBuilder::new(Backend::from_name("otel-http")?)?
        .with_header("X-CORRIDORA-KEY", rt.config("CORRIDORA_INGEST_KEY"))
        .with_url(format!("{}/v1/traces", base.trim_end_matches('/')).parse()?)
        .build()?;
    let provider = SdkTracerProvider::builder()
        .with_resource(tutorials::resource(&rt))
        .with_simple_exporter(CheckedExporter(exporter))
        .build();
    let headers = req
        .get_headers()
        .map(|(k, v)| (k.as_str().to_lowercase(), v.to_str().unwrap_or("").into()))
        .collect();
    let mut body = Vec::new();
    req.take_body().take(65537).read_to_end(&mut body)?;
    if body.len() > 65536 {
        return Ok(Response::from_status(413));
    }
    let path = req.get_url().path().to_owned()
        + &req
            .get_url()
            .query()
            .map(|q| format!("?{q}"))
            .unwrap_or_default();
    let reply = tutorials::handle(
        &mut rt,
        &provider,
        req.get_method_str(),
        &path,
        headers,
        &body,
    );
    if rt.config("ENABLE_PROVIDER_FORWARDING") == "true" {
        let cache = serde_json::from_slice::<Value>(&reply.body)
            .ok()
            .and_then(|v| v["cache"].as_str().map(str::to_owned))
            .unwrap_or("NONE".into());
        let native = json!({"timestamp":tutorials::now(),"event":"fastly.edge.request","status":reply.status,"cache":cache,"path":req.get_url().path(),"duration_ms":start.elapsed().unwrap_or_default().as_millis(),"traceId":reply.headers.get("x-trace-id"),"service_id":rt.config("FASTLY_SERVICE_ID"),"service_version":rt.config("SERVICE_VERSION"),"pop":std::env::var("FASTLY_POP").unwrap_or_default()});
        match fastly::log::Endpoint::try_from_name("corridora-edge") {
            Ok(mut endpoint) => {
                if writeln!(endpoint, "{}", native).is_err() {
                    eprintln!("Fastly log write failed");
                }
            }
            Err(_) => eprintln!("Fastly logging endpoint unavailable"),
        }
    }
    provider
        .force_flush()
        .map_err(|e| Error::msg(e.to_string()))?;
    provider.shutdown().map_err(|e| Error::msg(e.to_string()))?;
    let mut res = Response::from_status(reply.status)
        .with_header("Content-Type", &reply.content_type)
        .with_body(reply.body);
    for (k, v) in reply.headers {
        res.set_header(k, v);
    }
    Ok(res)
}
