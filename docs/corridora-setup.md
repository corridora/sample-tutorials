# Set up Corridora for Edge Notes

Published 2026-10-11 · Updated 2026-10-11

## Choose the destination and create keys

Contributors choose `CORRIDORA_TARGET=own` and their own Corridora organization.
An authorized maintained-demo operator chooses `demo`, which enforces environment
`demo`; canonical Demo UUID is `00000000-0000-4000-8000-000000000201`, but the key
routes the records. Never create Demo keys through the public demo persona.

In Corridora, create/select a project and its services:

| Service | Namespace | Role |
| --- | --- | --- |
| `sample-tutorials-web` | `corridora-samples.tutorials` | Browser RUM and framework actions |
| `sample-tutorials-api` | `corridora-samples.tutorials` | Rust search/progress/cache-fetch API |
| `sample-tutorials-fastly-adapter` | `corridora-samples.tutorials` | Fastly HTTPS logs and native stats |

Create a backend ingest key scoped for these services and a separate browser RUM
key scoped to the exact frontend origin. Browser keys are intentionally visible;
backend keys must remain in provider Secret Store or owner-only local files.
Management/query credentials are different and are not requested by this sample.
The sample never embeds maintained credentials.

## Run the setup wizard

```sh
./setup
# Or configure without starting:
./setup configure
./setup deploy --plan
./setup deploy                  # local install/build
./setup run
```

The wizard prompts for these groups:

| Answer | Default and effect |
| --- | --- |
| `SAMPLE_MODE` | `local`; `cloud` provisions a Fastly Compute service |
| `CORRIDORA_TARGET` | `own`; `demo` requires operator-supplied keys and sets environment `demo` |
| `CORRIDORA_OTLP_ENDPOINT` | `https://ingest.corridora.com`; base URL for all three signals |
| `CORRIDORA_RUM_SCRIPT_URL` | `https://rum.corridora.com/corridora-rum.js`; configurable distribution URL |
| `CORRIDORA_RUM_KEY` | Your origin-scoped browser key; becomes script `ckid` |
| `CORRIDORA_INGEST_KEY` | Masked backend key, exported as `X-CORRIDORA-KEY`; never in frontend output |
| `OTEL_SERVICE_NAME`, `OTEL_SERVICE_NAMESPACE` | API name/namespace from the service table |
| `DEPLOYMENT_ENVIRONMENT` | `development`, or enforced `demo` |
| `PORT` | `4106` for local HTTP |
| `DEMO_DAILY_START_UTC`, `DEMO_WINDOW_SECONDS` | `14:40`, `600`; every request derives scheduled state |
| `DEMO_CONTROL_TOKEN` | Masked, random if blank; authorizes expiring overrides and heartbeat |
| `ENABLE_SYNTHETICS` | `false`; opt in to daily GitHub workflow configuration |
| `ENABLE_PROVIDER_FORWARDING` | `false`; opt in to native HTTPS request logging |
| `PORTFOLIO_FIXTURE_URL` | Empty locally uses a seeded fixture; cloud requires controlled HTTPS `/api/fixture` |
| `DOWNSTREAM_AUTH_TOKEN` | Optional separate masked bearer token for your controlled origin |
| `TRACKER_SIMULATOR_URL` | Optional user-triggered controlled script; no real ad tags by default |
| `FASTLY_API_TOKEN`, `FASTLY_CUSTOMER_ID` | Scoped provisioning token and expected customer; customer discovered/checked before apply |
| `FASTLY_SERVICE_ID` | Explicit existing Compute service, or empty to create dedicated service |
| `FASTLY_RESOURCE_PREFIX` | `corridora-tutorials`; service/default `.edgecompute.app` domain name |
| `FASTLY_KV_STORE`, `FASTLY_CONFIG_STORE`, `FASTLY_SECRET_STORE` | New resource names; runtime links remain fixed `tutorial-progress`, `tutorial-config`, `tutorial-secrets` |
| `FASTLY_KV_ID`, `FASTLY_CONFIG_ID`, `FASTLY_SECRET_ID` | Explicit existing store IDs; selected resources survive teardown |
| `FASTLY_LOG_ADAPTER_URL`, `FASTLY_LOG_ADAPTER_TOKEN` | HTTPS receiver and dedicated masked bearer token when forwarding is enabled |
| `PUBLIC_URL` | Existing HTTPS custom domain, or provider default domain derived at deployment |
| `GITHUB_REPOSITORY` | `corridora/sample-tutorials`; published repo for opted-in daily runner and watchdog |

Never supply account passwords. For CI, use environment variables or a JSON file
with mode `0600` and `./setup configure --non-interactive --config <file>`.
Configuration is separated from secrets under `.sample/`; both files are mode
`0600` and Git-ignored. Secret values are sent as API request bodies/headers or
stdin to GitHub CLI, never command arguments. Scope Fastly provisioning rights to
the intended account/service as far as possible.

## Instrumentation and origins

`web/src/scripts/app.ts` loads the RUM distribution and awaits its initialization
before the first framework fetch. It supplies `ckid`, endpoint, web service,
namespace, environment, actual version, trace rate 1, long tasks and an exact
`data-trace-origins` allowlist. `@opentelemetry/api` supplies manual search/progress/
inspect action spans and explicit async context; RUM owns the browser SDK/provider.
Do not install a second browser provider. `runtime-config.js` contains only public
RUM settings. Same-origin requests are traced automatically by the real bundle.

`src/lib.rs` extracts W3C `traceparent`/`tracestate` before its Rust SDK SERVER span,
creates CLIENT spans for KV and the configured origin, and injects the CLIENT
context into that controlled outbound request. HTTP JSON OTLP traces use official
`opentelemetry-proto` transforms, logs carry matching trace/span IDs, and a gauge
exports bounded request duration. `src/bin/local.rs` sends via a 3-second local
HTTP transport; `src/bin/compute.rs` uses the pinned official Fastly exporter plus
[the documented transport completion patch](exporter-patch.md). The provider
force-flushes and shuts down inside the Compute request lifecycle. Native backends
set connection/first-byte/between-byte timeouts; errors are reported, without
instrumenting exporter HTTP requests. Fastly logs/metrics are explicit separate
pipelines; the Rust trace exporter does not implement them.

All frontend/API routes share one origin. API/control/diagnostic responses set
`Cache-Control: no-store`. The CSP derives exact RUM script/ingest/simulator origins
from saved configuration and blocks framing. There is no credentialed wildcard
CORS policy. Only the configured controlled fixture URL receives trace headers
and the independent downstream bearer token; export/control keys never do.
`Timing-Allow-Origin` on a simulator exposes resource timing and is separate from
trace propagation. Cache fetch CLIENT spans do not claim origin work on a cache
hit; origin service spans exist only when that service actually handles a request.

## Add Fastly native data

On your existing HTTPS adapter host (for example, the fleet's Akamai VM), set
`FASTLY_LOG_ADAPTER_TOKEN`, `CORRIDORA_OTLP_ENDPOINT`, `CORRIDORA_INGEST_KEY`,
`FASTLY_SERVICE_ID`, and `DEPLOYMENT_ENVIRONMENT` in a protected service environment.
Run `python3 scripts/provider_adapter.py logs`. Proxy only the receiver path from
your TLS host to `127.0.0.1:4319`; preserve its Authorization header and cap bodies
at 256 KB. Setup configures the Fastly HTTPS log endpoint with that dedicated
header when provider forwarding is enabled. Each actual edge request produces a
separate structured native log containing status/cache/POP/service identity.

For native metrics, use a separate read-only service-scoped `FASTLY_STATS_TOKEN`
and invoke `python3 scripts/provider_adapter.py stats` every 15 minutes. Only a
small allowlist of requests/errors/bytes/execution-time fields is converted. Use
`python3 scripts/provider_adapter.py replay` to retry retained failed log batches.
Protect/retain `.sample/adapter.sqlite`; successful log IDs and stats checkpoints
limit duplicate retries, with bounded failure storage. Delivery is at-least-once;
use `event.id` to recognize duplicates after uncertain HTTP acknowledgments.
Historical stats availability and field names depend on service/account entitlement.
Native log streaming is best effort. Hosting a new always-on adapter would violate
the zero incremental Fastly assumption; use the existing shared host or leave
native forwarding disabled and report that limitation.

## Verify correlation, incident and recovery

```sh
./setup verify
./setup scenario --phase regression --duration 120
./setup scenario --phase recover --duration 120
```

Verification prints a backend trace ID, runs an actual Playwright UI journey and
writes `test-results/evidence.json`. Open the new Corridora browser session and
find the actual fetch CLIENT span. Confirm the Rust SERVER span has the same
trace ID and `parent_span_id` equal to that CLIENT's span ID. Follow its CLIENT
origin call into Akamai when configured, and check any subsequent AWS fixture hop
in that app. Confirm the manual browser action, request log, duration metric and
separate native edge log/stats record. `/api/trace-check` with a supplied header
proves only backend propagation; ingest HTTP 200 proves only acknowledgment.

Locally, `RUM_BUNDLE_PATH=/path/to/actual/corridora-rum.js npm run test:browser`
routes the distribution URL to that actual bundle, captures OTLP on loopback with
mock keys, and asserts real CLIENT → SERVER parentage. Without a scoped Corridora
query credential this repository does not claim automatic verification of stored
Demo telemetry; those UI checks remain required. Do not point test captures at Demo.

The daily scenario changes `demo.release` and emits `event.name=deployment` or
`feature_flag`, `demo.simulated=true`, the effective scenario and actual build
version. Compare the seeded 1.5-second bypass against a healthy control. Expiring
manual overrides restore the UTC-derived state; a recovery override temporarily
suppresses the scheduled bad window. Verify the automatic recovery and posted
daily heartbeat; replay missing days through the bounded watchdog browser runner.

## Rerun, rotate and troubleshoot

Rerun `configure` to change keys/origins, then `deploy` to update saved provider
settings. Rotate backend/control/downstream secrets in Secret Store through the
wizard; revoke the old keys in Corridora/provider after verification. Browser RUM
key rotation updates public runtime config. Do not change `service.version` to
pretend a simulated deployment was a real binary release.

If RUM is absent, check the browser key scope, script `ckid`, CSP and the script
initialization status in browser devtools. If Rust export fails, check backend
TLS/SNI and `otel-http`, endpoint path `/v1/traces`, JSON content type and backend
key. If native forwarding is absent, check the separately provisioned HTTPS proxy,
dedicated token, Fastly endpoint, pending adapter batches and read-only stats
rights. If the bad action stays healthy, inspect `/api/demo/status`, UTC time and
KV consistency; wait for the persisted override to become visible before measuring.
If a custom hostname fails, complete Fastly's TLS/DNS challenge and check the
provider default hostname independently.

Review `./setup destroy --plan`, then use `--apply` to remove recorded owned
resources. Existing selected service/stores and the origin/adapter host survive.
For reused services, linked newly owned stores are conservatively retained rather
than deleting a resource still referenced by shared service versions; explicitly
unlink them in Fastly before removing them. Disable any external runner and
revoke unused keys. Local `destroy` clears seeded state and leaves owner-only
configuration available for deliberate credential rotation/removal.
