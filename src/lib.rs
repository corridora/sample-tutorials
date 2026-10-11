use opentelemetry::{
    KeyValue,
    propagation::{Extractor, Injector, TextMapPropagator},
    trace::{Span, SpanKind, TraceContextExt, Tracer, TracerProvider},
};
use opentelemetry_sdk::{Resource, propagation::TraceContextPropagator, trace::SdkTracerProvider};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

pub type Headers = HashMap<String, String>;
struct HeaderMap<'a>(&'a Headers);
impl Extractor for HeaderMap<'_> {
    fn get(&self, k: &str) -> Option<&str> {
        self.0.get(k).map(String::as_str)
    }
    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(String::as_str).collect()
    }
}
struct HeaderInject<'a>(&'a mut Headers);
impl Injector for HeaderInject<'_> {
    fn set(&mut self, k: &str, v: String) {
        self.0.insert(k.into(), v);
    }
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn attrs(v: &[(&str, Value)]) -> Vec<Value> {
    v.iter().map(|(k,v)| json!({"key":k,"value":if let Some(b)=v.as_bool(){json!({"boolValue":b})}else if let Some(n)=v.as_u64(){json!({"intValue":n.to_string()})}else{json!({"stringValue":v.as_str().unwrap_or_default()})}})).collect()
}

pub trait Runtime {
    fn config(&self, k: &str) -> String;
    fn read(&mut self, k: &str) -> Option<String>;
    fn write(&mut self, k: &str, v: &str) -> Result<(), String>;
    fn origin(
        &mut self,
        url: &str,
        headers: Headers,
        pass: bool,
    ) -> Result<(Value, String), String>;
    fn send_signal(&mut self, signal: &str, payload: Value) -> Result<(), String>;
    fn static_file(&self, path: &str) -> Option<(Vec<u8>, String)>;
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Scenario {
    pub phase: String,
    pub expires_at: u64,
    pub release: String,
}
#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
    pub headers: Headers,
}
impl Reply {
    pub fn json(status: u16, v: Value) -> Self {
        Self {
            status,
            content_type: "application/json".into(),
            body: v.to_string().into_bytes(),
            headers: Headers::new(),
        }
    }
}

pub fn resource(r: &impl Runtime) -> Resource {
    Resource::builder_empty()
        .with_attributes([
            KeyValue::new("service.name", r.config("OTEL_SERVICE_NAME")),
            KeyValue::new("service.namespace", r.config("OTEL_SERVICE_NAMESPACE")),
            KeyValue::new("service.version", r.config("SERVICE_VERSION")),
            KeyValue::new(
                "deployment.environment.name",
                r.config("DEPLOYMENT_ENVIRONMENT"),
            ),
            KeyValue::new("cloud.provider", "fastly"),
            KeyValue::new("cloud.availability_zone", r.config("FASTLY_POP")),
            KeyValue::new("cloud.region", r.config("FASTLY_REGION")),
            KeyValue::new("cloud.platform", "fastly.compute"),
            KeyValue::new("cloud.account.id", r.config("FASTLY_CUSTOMER_ID")),
            KeyValue::new("cloud.resource_id", r.config("FASTLY_SERVICE_ID")),
        ])
        .build()
}
pub fn resource_json(r: &impl Runtime) -> Value {
    json!({"attributes":attrs(&[("service.name",json!(r.config("OTEL_SERVICE_NAME"))),("service.namespace",json!(r.config("OTEL_SERVICE_NAMESPACE"))),("service.version",json!(r.config("SERVICE_VERSION"))),("deployment.environment.name",json!(r.config("DEPLOYMENT_ENVIRONMENT"))),("cloud.provider",json!("fastly")),("cloud.resource_id",json!(r.config("FASTLY_SERVICE_ID")))])})
}
pub fn effective(r: &mut impl Runtime, t: u64) -> Scenario {
    let stored = r
        .read("scenario")
        .and_then(|s| serde_json::from_str::<Scenario>(&s).ok());
    if let Some(s) = stored {
        if s.expires_at > t {
            return s;
        }
    }
    let start = r.config("DEMO_DAILY_START_UTC");
    let parts: Vec<_> = start.split(':').collect();
    let sec = parts
        .first()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(14)
        * 3600
        + parts
            .get(1)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(40)
            * 60;
    let dur = r
        .config("DEMO_WINDOW_SECONDS")
        .parse::<u64>()
        .unwrap_or(600)
        .clamp(1, 600);
    let today = t - t % 86400;
    let scheduled_start = today + sec;
    let latest_start = if t >= scheduled_start {
        Some(scheduled_start)
    } else {
        scheduled_start.checked_sub(86400)
    };
    if let Some(window_start) = latest_start.filter(|&window_start| t - window_start < dur) {
        let incident_day = window_start - window_start % 86400;
        Scenario {
            phase: "regression".into(),
            expires_at: window_start + dur,
            release: format!("daily-cache-regression-{incident_day}"),
        }
    } else {
        Scenario {
            phase: "healthy".into(),
            expires_at: 0,
            release: "baseline".into(),
        }
    }
}
fn secret_eq(a: &str, b: &str) -> bool {
    let mut d = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        d |= (a.as_bytes().get(i).copied().unwrap_or(0) ^ b.as_bytes().get(i).copied().unwrap_or(0))
            as usize;
    }
    d == 0 && !a.is_empty()
}
fn emit(
    r: &mut impl Runtime,
    event: &str,
    s: &Scenario,
    trace: &str,
    span: &str,
    duration: f64,
    status: u16,
) {
    let ns = format!("{}000000000", now());
    let attributes = attrs(&[
        ("event.name", json!(event)),
        ("demo.simulated", json!(true)),
        ("demo.scenario", json!(s.phase)),
        ("demo.release", json!(s.release)),
        ("http.response.status_code", json!(status)),
    ]);
    let log = json!({"resourceLogs":[{"resource":resource_json(r),"scopeLogs":[{"scope":{"name":"corridora.sample-tutorials"},"logRecords":[{"timeUnixNano":ns,"severityNumber":9,"severityText":"INFO","body":{"stringValue":event},"attributes":attributes,"traceId":trace,"spanId":span}]}]}]});
    if let Err(e) = r.send_signal("logs", log) {
        eprintln!("OTLP logs failed: {e}");
    }
    let metric = json!({"resourceMetrics":[{"resource":resource_json(r),"scopeMetrics":[{"scope":{"name":"corridora.sample-tutorials"},"metrics":[{"name":"tutorial.request.duration","unit":"s","gauge":{"dataPoints":[{"timeUnixNano":ns,"asDouble":duration,"attributes":attributes}]}}]}]}]});
    if let Err(e) = r.send_signal("metrics", metric) {
        eprintln!("OTLP metrics failed: {e}");
    }
}
pub fn handle(
    r: &mut impl Runtime,
    provider: &SdkTracerProvider,
    method: &str,
    url: &str,
    headers: Headers,
    body: &[u8],
) -> Reply {
    let start = SystemTime::now();
    let u = url::Url::parse(&format!("http://sample{url}")).unwrap();
    let path = u.path();
    let parent = TraceContextPropagator::new().extract(&HeaderMap(&headers));
    let tracer = provider.tracer("corridora.sample-tutorials");
    let mut span = tracer
        .span_builder(format!("{method} {path}"))
        .with_kind(SpanKind::Server)
        .start_with_context(&tracer, &parent);
    span.set_attribute(KeyValue::new("http.request.method", method.to_owned()));
    span.set_attribute(KeyValue::new("url.path", path.to_owned()));
    let cx = parent.with_span(span);
    let sc = cx.span().span_context().clone();
    let trace = sc.trace_id().to_string();
    let span_id = sc.span_id().to_string();
    let mut scenario = effective(r, now());
    let reply = (|| match (method, path) {
        ("GET", "/healthz") => Reply::json(
            200,
            json!({"ok":true,"service":"sample-tutorials","exporter_configured":!r.config("CORRIDORA_INGEST_KEY").is_empty()}),
        ),
        ("GET", "/api/trace-check") => {
            Reply::json(200, json!({"trace_id":trace,"span_id":span_id}))
        }
        ("GET", "/api/demo/status") => Reply::json(
            200,
            json!({"phase":scenario.phase,"release":scenario.release,"expires_at":scenario.expires_at,"service_version":r.config("SERVICE_VERSION"),"heartbeat":r.read("heartbeat").and_then(|s|serde_json::from_str::<Value>(&s).ok())}),
        ),
        ("POST", "/api/demo/scenario") => {
            let token = r.config("DEMO_CONTROL_TOKEN");
            if !secret_eq(
                headers
                    .get("authorization")
                    .map(String::as_str)
                    .unwrap_or(""),
                &format!("Bearer {token}"),
            ) || token.is_empty()
            {
                return Reply::json(401, json!({"error":"unauthorized"}));
            }
            let v: Value = match serde_json::from_slice(body) {
                Ok(v) => v,
                Err(_) => return Reply::json(400, json!({"error":"invalid JSON"})),
            };
            let phase = v["phase"].as_str().unwrap_or("");
            let dur = v["duration_seconds"].as_u64().unwrap_or(0);
            if !["healthy", "regression", "recover"].contains(&phase) || !(1..=600).contains(&dur) {
                return Reply::json(
                    400,
                    json!({"error":"phase healthy/regression/recover and integer duration_seconds 1..600 required"}),
                );
            }
            scenario = Scenario {
                phase: if phase == "recover" { "healthy" } else { phase }.into(),
                expires_at: now() + dur,
                release: format!("mock-deploy-{phase}-{}-{}", now(), span_id),
            };
            if let Err(e) = r.write("scenario", &serde_json::to_string(&scenario).unwrap()) {
                return Reply::json(503, json!({"error":e}));
            }
            emit(r, "deployment", &scenario, &trace, &span_id, 0., 200);
            Reply::json(200, serde_json::to_value(&scenario).unwrap())
        }
        ("POST", "/api/demo/heartbeat") => {
            let token = r.config("DEMO_CONTROL_TOKEN");
            if token.is_empty()
                || !secret_eq(
                    headers
                        .get("authorization")
                        .map(String::as_str)
                        .unwrap_or(""),
                    &format!("Bearer {token}"),
                )
            {
                return Reply::json(401, json!({"error":"unauthorized"}));
            }
            let v: Value = serde_json::from_slice(body).unwrap_or(json!({}));
            if v["result"] != "passed" || v["trace_id"].as_str().unwrap_or("").len() != 32 {
                return Reply::json(400, json!({"error":"passed result and trace_id required"}));
            }
            let h = json!({"at":now(),"result":"passed","trace_id":v["trace_id"]});
            match r.write("heartbeat", &h.to_string()) {
                Ok(_) => Reply::json(200, h),
                Err(e) => Reply::json(503, json!({"error":e})),
            }
        }
        ("GET", "/api/search") => {
            let q = u
                .query_pairs()
                .find(|(k, _)| k == "q")
                .map(|(_, v)| v.to_lowercase())
                .unwrap_or_default();
            let child = tracer
                .span_builder("kv tutorial.search")
                .with_kind(SpanKind::Client)
                .start_with_context(&tracer, &cx);
            let cc = cx.with_span(child);
            let data = r
                .read("catalog")
                .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                .unwrap_or_else(catalog);
            cc.span().end();
            let items: Vec<Value> = data
                .as_array()
                .unwrap()
                .iter()
                .filter(|v| {
                    v["title"]
                        .as_str()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&q)
                })
                .cloned()
                .collect();
            Reply::json(200, json!({"items":items,"trace_id":trace}))
        }
        ("GET", "/api/progress") => Reply::json(
            200,
            json!({"completed":r.read("progress:demo-learner").and_then(|s|serde_json::from_str::<Value>(&s).ok()).unwrap_or(json!([])),"trace_id":trace}),
        ),
        ("POST", "/api/progress") => {
            let v: Value = serde_json::from_slice(body).unwrap_or(json!({}));
            let id = v["tutorial_id"].as_str().unwrap_or("");
            if !["edge-cache", "trace-context", "async-rust"].contains(&id) {
                return Reply::json(400, json!({"error":"unknown seeded tutorial"}));
            }
            let child = tracer
                .span_builder("kv progress.write")
                .with_kind(SpanKind::Client)
                .start_with_context(&tracer, &cx);
            let cc = cx.with_span(child);
            let mut ids: Vec<String> = r
                .read("progress:demo-learner")
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
            if !ids.iter().any(|s| s == id) {
                ids.push(id.into());
            }
            let res = r.write(
                "progress:demo-learner",
                &serde_json::to_string(&ids).unwrap(),
            );
            cc.span().end();
            match res {
                Ok(_) => Reply::json(200, json!({"completed":ids,"trace_id":trace})),
                Err(e) => Reply::json(503, json!({"error":e})),
            }
        }
        ("GET", "/api/origin") => {
            let affected = u
                .query_pairs()
                .any(|(k, v)| k == "tutorial_id" && v == "edge-cache");
            let bad = affected && scenario.phase == "regression";
            let mut endpoint = r.config("PORTFOLIO_FIXTURE_URL");
            if endpoint.is_empty() {
                endpoint = "local://fixture".into();
            } else if bad {
                endpoint.push_str(if endpoint.contains('?') {
                    "&slow=1"
                } else {
                    "?slow=1"
                });
            }
            let child = tracer
                .span_builder("GET fixture.fetch")
                .with_kind(SpanKind::Client)
                .with_attributes([KeyValue::new("demo.cache.pass", bad)])
                .start_with_context(&tracer, &cx);
            let cc = cx.with_span(child);
            let mut outbound = Headers::new();
            TraceContextPropagator::new().inject_context(&cc, &mut HeaderInject(&mut outbound));
            let auth = r.config("DOWNSTREAM_AUTH_TOKEN");
            if !auth.is_empty() {
                outbound.insert("authorization".into(), format!("Bearer {auth}"));
            }
            let res = r.origin(&endpoint, outbound, bad);
            cc.span().end();
            match res {
                Ok((v, cache)) => Reply::json(
                    200,
                    json!({"fixture":v,"cache":cache,"phase":scenario.phase,"affected":bad,"trace_id":trace,"span_id":span_id}),
                ),
                Err(_) => Reply::json(
                    502,
                    json!({"error":"bounded fixture call failed","trace_id":trace}),
                ),
            }
        }
        _ if method == "GET" && !path.starts_with("/api/") => match r.static_file(path) {
            Some((b, ct)) => Reply {
                status: 200,
                content_type: ct,
                body: b,
                headers: Headers::new(),
            },
            None => Reply::json(404, json!({"error":"not found"})),
        },
        _ => Reply::json(404, json!({"error":"not found"})),
    })();
    cx.span().set_attribute(KeyValue::new(
        "http.response.status_code",
        reply.status as i64,
    ));
    cx.span()
        .set_attribute(KeyValue::new("demo.scenario", scenario.phase.clone()));
    cx.span()
        .set_attribute(KeyValue::new("demo.release", scenario.release.clone()));
    let previous = r.read("last-release");
    if previous.as_deref() != Some(&scenario.release) {
        emit(
            r,
            "feature_flag",
            &scenario,
            &trace,
            &span_id,
            0.,
            reply.status,
        );
        let _ = r.write("last-release", &scenario.release);
    }
    emit(
        r,
        "request",
        &scenario,
        &trace,
        &span_id,
        start.elapsed().unwrap_or_default().as_secs_f64(),
        reply.status,
    );
    if reply.status >= 500 {
        cx.span()
            .set_status(opentelemetry::trace::Status::error("request failed"));
    }
    cx.span().end();
    let mut reply = reply;
    reply
        .headers
        .insert("cache-control".into(), "no-store".into());
    reply.headers.insert("x-trace-id".into(), trace);
    let mut scripts = vec!["'self'".to_owned()];
    for key in ["CORRIDORA_RUM_SCRIPT_URL", "TRACKER_SIMULATOR_URL"] {
        if let Ok(u) = url::Url::parse(&r.config(key)) {
            scripts.push(u.origin().ascii_serialization());
        }
    }
    let ingest = url::Url::parse(&r.config("CORRIDORA_OTLP_ENDPOINT"))
        .map(|u| u.origin().ascii_serialization())
        .unwrap_or_else(|_| "'self'".into());
    reply.headers.insert("content-security-policy".into(),format!("default-src 'self'; script-src {}; connect-src 'self' {}; style-src 'self' 'unsafe-inline'; img-src 'self' data:; base-uri 'none'; frame-ancestors 'none'",scripts.join(" "),ingest));
    reply
        .headers
        .insert("x-content-type-options".into(), "nosniff".into());
    reply
}
pub fn catalog() -> Value {
    json!([{"id":"edge-cache","title":"Understand edge caching","minutes":8},{"id":"trace-context","title":"Follow W3C trace context","minutes":6},{"id":"async-rust","title":"Write bounded async Rust","minutes":10}])
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Mem {
        cfg: Headers,
        store: Headers,
    }
    impl Runtime for Mem {
        fn config(&self, k: &str) -> String {
            self.cfg.get(k).cloned().unwrap_or_default()
        }
        fn read(&mut self, k: &str) -> Option<String> {
            self.store.get(k).cloned()
        }
        fn write(&mut self, k: &str, v: &str) -> Result<(), String> {
            self.store.insert(k.into(), v.into());
            Ok(())
        }
        fn origin(&mut self, _: &str, h: Headers, p: bool) -> Result<(Value, String), String> {
            assert!(h.contains_key("traceparent"));
            Ok((json!({"pass":p}), "fixture".into()))
        }
        fn send_signal(&mut self, _: &str, v: Value) -> Result<(), String> {
            assert!(v.is_object());
            Ok(())
        }
        fn static_file(&self, _: &str) -> Option<(Vec<u8>, String)> {
            None
        }
    }
    fn mem() -> Mem {
        Mem {
            cfg: Headers::from([
                ("DEMO_CONTROL_TOKEN".into(), "test-control".into()),
                ("DEMO_DAILY_START_UTC".into(), "14:40".into()),
            ]),
            store: Headers::new(),
        }
    }
    #[test]
    fn daily_and_expiry() {
        let mut m = mem();
        assert_eq!(effective(&mut m, 14 * 3600 + 45 * 60).phase, "regression");
        assert_eq!(effective(&mut m, 14 * 3600 + 51 * 60).phase, "healthy");
        m.write(
            "scenario",
            r#"{"phase":"regression","expires_at":100,"release":"test"}"#,
        )
        .unwrap();
        assert_eq!(effective(&mut m, 99).release, "test");
        assert_eq!(effective(&mut m, 101).phase, "healthy");
    }
    #[test]
    fn daily_crosses_midnight() {
        let mut m = mem();
        m.cfg.insert("DEMO_DAILY_START_UTC".into(), "23:55".into());
        m.cfg.insert("DEMO_WINDOW_SECONDS".into(), "600".into());
        let day = 20_000 * 86400;
        let start = day + 23 * 3600 + 55 * 60;
        let end = start + 600;
        assert_eq!(effective(&mut m, start - 1).phase, "healthy");
        let incident = effective(&mut m, start);
        assert_eq!(incident.phase, "regression");
        assert_eq!(incident.expires_at, end);
        for timestamp in [day + 86400, end - 1] {
            let after_midnight = effective(&mut m, timestamp);
            assert_eq!(after_midnight.phase, "regression");
            assert_eq!(after_midnight.release, incident.release);
            assert_eq!(after_midnight.expires_at, end);
        }
        assert_eq!(effective(&mut m, end).phase, "healthy");
    }
    #[test]
    fn auth_and_duration() {
        let mut m = mem();
        let p = SdkTracerProvider::builder().build();
        let a = handle(
            &mut m,
            &p,
            "POST",
            "/api/demo/scenario",
            Headers::new(),
            b"{}",
        );
        assert_eq!(a.status, 401);
        let h = Headers::from([("authorization".into(), "Bearer test-control".into())]);
        assert_eq!(
            handle(
                &mut m,
                &p,
                "POST",
                "/api/demo/scenario",
                h.clone(),
                br#"{"phase":"regression","duration_seconds":601}"#
            )
            .status,
            400
        );
        assert_eq!(
            handle(
                &mut m,
                &p,
                "POST",
                "/api/demo/scenario",
                h,
                br#"{"phase":"regression","duration_seconds":120}"#
            )
            .status,
            200
        );
    }
    #[test]
    fn w3c_trace() {
        let mut m = mem();
        let p = SdkTracerProvider::builder().build();
        let h = Headers::from([(
            "traceparent".into(),
            "00-11111111111111111111111111111111-2222222222222222-01".into(),
        )]);
        let a = handle(&mut m, &p, "GET", "/api/trace-check", h, b"");
        assert!(
            String::from_utf8(a.body)
                .unwrap()
                .contains("11111111111111111111111111111111")
        );
    }
    #[test]
    fn progress() {
        let mut m = mem();
        let p = SdkTracerProvider::builder().build();
        assert_eq!(
            handle(
                &mut m,
                &p,
                "POST",
                "/api/progress",
                Headers::new(),
                br#"{"tutorial_id":"edge-cache"}"#
            )
            .status,
            200
        );
        assert!(
            String::from_utf8(handle(&mut m, &p, "GET", "/api/progress", Headers::new(), b"").body)
                .unwrap()
                .contains("edge-cache")
        );
    }
}
