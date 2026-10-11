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
    trace::{SdkTracerProvider, SpanData, SpanExporter},
};
use serde_json::{Value, json};
use std::io::Read;
use std::{collections::HashMap, fs, path::PathBuf, time::Duration};
use tutorials::{Headers, Runtime};
#[derive(Debug)]
struct Exporter {
    endpoint: String,
    key: String,
    resource: ResourceAttributesWithSchema,
}
impl SpanExporter for Exporter {
    fn set_resource(&mut self, r: &Resource) {
        self.resource = r.into();
    }
    fn export(&self, b: Vec<SpanData>) -> impl std::future::Future<Output = OTelSdkResult> + Send {
        let v = ExportTraceServiceRequest {
            resource_spans: group_spans_by_resource_and_scope(b, &self.resource),
        };
        let result = send(
            &self.endpoint,
            &self.key,
            "traces",
            serde_json::to_value(v).unwrap(),
        )
        .map_err(OTelSdkError::InternalFailure);
        if result.is_err() {
            eprintln!("OTLP traces failed (endpoint/key/status); request bounded to 3 seconds");
        }
        std::future::ready(result)
    }
}
fn send(base: &str, key: &str, signal: &str, v: Value) -> Result<(), String> {
    if key.is_empty() {
        return Ok(());
    }
    let response = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(3))
        .build()
        .post(&format!("{}/v1/{signal}", base.trim_end_matches('/')))
        .set("X-CORRIDORA-KEY", key)
        .send_json(v)
        .map_err(|_| format!("{signal} export failed"))?;
    if response.status() / 100 != 2 {
        return Err(format!("{signal} ingest rejected"));
    }
    Ok(())
}
struct Local {
    root: PathBuf,
    cfg: HashMap<String, String>,
}
impl Runtime for Local {
    fn config(&self, k: &str) -> String {
        self.cfg.get(k).cloned().unwrap_or_default()
    }
    fn read(&mut self, k: &str) -> Option<String> {
        fs::read_to_string(self.root.join(k.replace(':', "-"))).ok()
    }
    fn write(&mut self, k: &str, v: &str) -> Result<(), String> {
        fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
        let p = self.root.join(k.replace(':', "-"));
        let tmp = p.with_extension("tmp");
        fs::write(&tmp, v)
            .and_then(|_| fs::rename(tmp, p))
            .map_err(|e| e.to_string())
    }
    fn origin(&mut self, u: &str, h: Headers, pass: bool) -> Result<(Value, String), String> {
        if u.starts_with("local://") {
            if pass {
                std::thread::sleep(Duration::from_millis(1500));
            }
            return Ok((
                json!({"fictional":true,"holdings":[{"symbol":"DEMO","quantity":2}],"valuation":85}),
                if pass { "PASS" } else { "LOCAL_FIXTURE" }.into(),
            ));
        }
        let mut req = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(3))
            .build()
            .get(u);
        for (k, v) in h {
            req = req.set(&k, &v);
        }
        if pass {
            req = req.set("Cache-Control", "no-cache");
        }
        let res = req.call().map_err(|_| "origin request failed")?;
        let v = res.into_json().map_err(|_| "origin JSON invalid")?;
        Ok((v, if pass { "PASS" } else { "NETWORK" }.into()))
    }
    fn send_signal(&mut self, s: &str, v: Value) -> Result<(), String> {
        send(
            &self.config("CORRIDORA_OTLP_ENDPOINT"),
            &self.config("CORRIDORA_INGEST_KEY"),
            s,
            v,
        )
    }
    fn static_file(&self, path: &str) -> Option<(Vec<u8>, String)> {
        if path == "/runtime-config.js" {
            return Some((
                runtime_script(self).into_bytes(),
                "application/javascript".into(),
            ));
        }
        let path = if path == "/" { "/index.html" } else { path };
        if path.contains("..") || path.contains('\\') {
            return None;
        }
        let b = fs::read(format!("web/dist{path}")).ok()?;
        let ct = if path.ends_with(".js") {
            "application/javascript"
        } else if path.ends_with(".css") {
            "text/css"
        } else {
            "text/html"
        };
        Some((b, ct.into()))
    }
}
fn runtime_script(r: &impl Runtime) -> String {
    format!(
        "window.SAMPLE_CONFIG={};",
        json!({"rumKey":r.config("CORRIDORA_RUM_KEY"),"rumScript":r.config("CORRIDORA_RUM_SCRIPT_URL"),"endpoint":r.config("CORRIDORA_OTLP_ENDPOINT"),"namespace":r.config("OTEL_SERVICE_NAMESPACE"),"environment":r.config("DEPLOYMENT_ENVIRONMENT"),"version":r.config("SERVICE_VERSION"),"trackerUrl":r.config("TRACKER_SIMULATOR_URL")})
    )
}
fn main() {
    let mut cfg: HashMap<String, String> = std::env::vars().collect();
    for (k, v) in [
        ("PORT", "4106"),
        ("SERVICE_VERSION", env!("SAMPLE_BUILD_VERSION")),
        ("OTEL_SERVICE_NAME", "sample-tutorials-api"),
        ("OTEL_SERVICE_NAMESPACE", "corridora-samples.tutorials"),
        ("DEPLOYMENT_ENVIRONMENT", "development"),
        ("DEMO_DAILY_START_UTC", "14:40"),
        (
            "CORRIDORA_RUM_SCRIPT_URL",
            "https://rum.corridora.com/corridora-rum.js",
        ),
    ] {
        cfg.entry(k.into()).or_insert(v.into());
    }
    let mut rt = Local {
        root: PathBuf::from(
            cfg.get("LOCAL_STATE_DIR")
                .map(String::as_str)
                .unwrap_or(".sample/store"),
        ),
        cfg,
    };
    let provider = SdkTracerProvider::builder()
        .with_resource(tutorials::resource(&rt))
        .with_simple_exporter(Exporter {
            endpoint: rt.config("CORRIDORA_OTLP_ENDPOINT"),
            key: rt.config("CORRIDORA_INGEST_KEY"),
            resource: ResourceAttributesWithSchema::default(),
        })
        .build();
    let port = rt.config("PORT");
    let server = tiny_http::Server::http(format!("127.0.0.1:{port}")).expect("bind local HTTP");
    println!("Tutorials http://127.0.0.1:{port}");
    for mut req in server.incoming_requests() {
        let method = req.method().as_str().to_owned();
        let url = req.url().to_owned();
        let headers = req
            .headers()
            .iter()
            .map(|h| {
                (
                    h.field.as_str().as_str().to_lowercase(),
                    h.value.as_str().to_owned(),
                )
            })
            .collect();
        let mut body = vec![];
        if let Err(_) = std::io::Read::take(req.as_reader(), 65537).read_to_end(&mut body) {
            continue;
        }
        if body.len() > 65536 {
            let _ = req.respond(tiny_http::Response::empty(413));
            continue;
        }
        let reply = tutorials::handle(&mut rt, &provider, &method, &url, headers, &body);
        let mut response = tiny_http::Response::from_data(reply.body)
            .with_status_code(reply.status)
            .with_header(
                tiny_http::Header::from_bytes("Content-Type", reply.content_type).unwrap(),
            );
        for (k, v) in reply.headers {
            response = response.with_header(tiny_http::Header::from_bytes(k, v).unwrap());
        }
        let _ = req.respond(response);
        if let Err(e) = provider.force_flush() {
            eprintln!("trace flush failed: {e}");
        }
    }
}
