# Edge Notes: Astro + Rust on Fastly

Published 2026-10-11 · Updated 2026-10-11

A small tutorial search and progress notebook, built to make an edge regression
visible in Corridora. Astro prebuilds the UI. Rust runs the API locally or as a
Fastly Compute Wasm package. Three fictional lessons and one shared synthetic
learner keep the dataset tiny. This is a public demo, so progress is intentionally
shared; do not use personal information or real accounts.

```text
Astro UI + Corridora RUM → Rust SERVER span → KV search/progress
                                         → Fastly cache fetch → controlled Akamai fixture
SDK traces + explicit OTLP logs/metrics → Corridora
Fastly HTTPS edge request logs → Python adapter → OTLP logs
Fastly historical stats API → Python adapter → OTLP metrics
```

## Start locally

Python 3.11+, Node 24+, Rust 1.86+, and a browser are required. The standalone local
Rust runtime uses files in `.sample/store` in place of Fastly stores and a bounded
1.5-second origin fixture in place of Akamai. No cloud account is needed.

```sh
./setup                          # prompts, builds, starts the local app
# In another terminal:
./setup verify
./setup scenario --phase regression --duration 120
./setup scenario --phase recover --duration 120
```

Open http://127.0.0.1:4106. Search for “edge”, mark a lesson complete, inspect the
edge cache origin, then compare the healthy control. `./setup --help` uses only
Python's standard library. For separate repeatable steps:

| Command | Local behavior | Cloud behavior |
| --- | --- | --- |
| `./setup configure` | Prompt and save owner-only answers | Prompt for Fastly account/service/stores, origin, optional adapter and GitHub schedule |
| `./setup deploy --dry-run` | Show plan without installs | Show resources/account/cost without API requests |
| `./setup deploy` | Install locked dependencies and build | Use `--apply` after plan review to provision/update and activate a service |
| `./setup run` | Run the Rust process; Ctrl-C stops it | Check health and print deployed URL |
| `./setup verify` | Exercise the running app in Playwright | Exercise the hosted app in Playwright |
| `./setup scenario` | Expiring authenticated override | Same endpoint, stored in Fastly KV |
| `./setup destroy --dry-run` | Show local state cleanup | Show exact owned inventory and preserved shared resources |
| `./setup destroy --apply` | Clear local seeded state | Delete owned service/stores; retain selected existing resources |

Noninteractive configuration reads `.env.example` names from the environment or
`./setup configure --non-interactive --config /path/to/answers.json`. JSON config
must have mode `0600`. Do not put secrets in command arguments. Saved configuration
and secrets live in separate Git-ignored `.sample/*.json` files with mode `0600`;
the directory has mode `0700`. Empty optional URLs use local fixtures. The setup
script generates a control token when absent. Contributor setup defaults to their
own Corridora organization; maintained Demo credentials must be supplied by an
operator. See [Set up Corridora](docs/corridora-setup.md) for every prompt, keys,
RUM correlation and troubleshooting.

## Deploy to Fastly

See [deployment details](docs/deployment.md). `./setup deploy --apply` performs real
Fastly API calls, builds the Wasm package, seeds KV, links stores, updates config
and secrets, configures bounded TLS backends, uploads the package and activates a
version. It records every newly owned resource after creation so a rerun resumes.
It refuses to silently adopt an existing store with the same name; select its ID
explicitly. A selected existing service is never deleted.

Cloud mode needs a controlled HTTPS fixture origin supporting `?slow=1`, such as
the Akamai sample's `/api/fixture`; this avoids a busy loop or an unbounded edge
sleep. `scripts/fixture_origin.py` is a standalone origin reference if the other
sample is unavailable. Local quick start never depends on another app. Cache
responses at the controlled origin must have `Cache-Control: public, max-age=60`.
During the seeded bad revision the Fastly client calls `set_pass(true)` and adds
`slow=1`. Every origin hop has a 3-second first-byte and 1-second between-byte
limit. Warm the same URL from the same synthetic location before comparing;
healthy cache hits remain opportunistic. `UNKNOWN` is reported honestly when an
origin/cache response lacks a cache-state header. No origin SERVER span is
fabricated for a cache hit; the SDK CLIENT span describes the cache fetch.

The default hostname is `<resource-prefix>.edgecompute.app`; Fastly must accept
that domain and route it to this service. For an existing custom domain, use an
already provisioned TLS domain and set `PUBLIC_URL`. Setup creates the service
domain but does not purchase a domain or provision a TLS subscription. Follow
Fastly's displayed DNS/TLS challenge before declaring the custom hostname live.

## Daily incident and recovery

The effective bad window is **14:40–14:50 UTC**, derived on every request, even if
a scheduler misses a timer. Only `tutorial_id=edge-cache` is affected. The healthy
`trace-context` control remains fast. Overrides accept only `healthy`, `regression`
or `recover`, with integer `duration_seconds` from 1 to 600. The bearer control
token is required and empty tokens fail closed. On expiry the handler returns to
its current UTC-derived state; during the daily window this can still be the
scheduled regression. `demo.release` identifies a simulated rollout;
`service.version` is the actual source/commit build identity.

`tests/journey.spec.mjs` searches, saves progress, measures a healthy origin,
activates the bounded slowdown, checks the healthy control, checks override expiry,
recovers, and posts a protected heartbeat containing a real browser request trace
ID. The daily GitHub Actions workflow runs at 14:42 UTC, with a watchdog at 18:00.
A missing same-day heartbeat causes the **full** bad-action journey to replay.
With `ENABLE_SYNTHETICS=true`, cloud setup uses an authenticated `gh` session to
set the dedicated repo's `SAMPLE_URL` and `DEMO_CONTROL_TOKEN` secrets, enable the
workflow and set its opt-in variable. The repository/workflow must already be
published. Teardown disables the variable it enabled. Fastly Compute has no native
cron; GitHub schedules can be delayed/dropped and public schedules disable after
60 days of inactivity. For guaranteed daily evidence, invoke `node scripts/daily.mjs
--watchdog` from an existing persistent runner too. Failures produce retained
2-day artifacts and a missing heartbeat visible at `/api/demo/status`.

## Provider telemetry and controlled trackers

`ENABLE_PROVIDER_FORWARDING=true` sends a separate actual edge-request record to
the configured `corridora-edge` HTTPS log endpoint: status, cache observation, POP,
path, duration, Fastly service identity and trace ID. It does not relabel duplicate
application events as native provider data. The Python adapter receives NDJSON,
checks a dedicated bearer token and converts records to OTLP logs. A SQLite ledger
deduplicates successful records for 24 hours, bounds retained IDs to 10,000, and
retains at most 32 failed batches for a day. Run its `replay` mode to retry them.
This remains at-least-once delivery: an ingest timeout after acceptance can produce
a duplicate, identified by stable `event.id`. Stats polling checkpoints successful
intervals. Run every 15 minutes with a read-only, service-scoped `FASTLY_STATS_TOKEN`.

The adapter listens on loopback; place it behind your existing authenticated HTTPS
proxy/shared adapter host. That host is an explicit dependency and its incremental
cost is outside Fastly's zero-within-quota claim. It can run on the fleet's Akamai
VM. Fastly log streaming is best effort; unavailable logs do not block app requests.
The direct application pipeline sends traces, logs and a request-duration metric
independently. Export errors are visible on stderr and never recursively traced.

`TRACKER_SIMULATOR_URL` enables a user-triggered controlled cross-origin script.
It should expose delayed script, failed pixel, a bounded 250-ms task and layout
shift, plus `Timing-Allow-Origin` for this exact UI origin. Only your configured
origin receives dependency trace context. Real Meta/Google advertising tags are
not loaded by this sample; use isolated properties and explicit consent if adding
them later. Browser timing does not prove receipt by a vendor or access to vendor
backend traces. Backend keys are never browser configuration.

## Validation

```sh
npm ci --ignore-scripts
npm run build
cargo test --locked
cargo build --locked --bin local
cargo check --locked --features compute --bin sample-tutorials
python3 -m unittest discover -s tests -p 'test_*.py'
npx playwright install chromium
npm run test:browser
# Optional: use an actual locally available Corridora RUM bundle; no fake tracer:
RUM_BUNDLE_PATH=/absolute/path/corridora-rum.js npm run test:browser
# With official rustup toolchain + target:
rustup target add wasm32-wasip1 --toolchain 1.86.0
cargo +1.86.0 build --locked --release --features compute --bin sample-tutorials --target wasm32-wasip1
```

The optional local RUM test routes the script CDN URL to your supplied real bundle,
collects OTLP with mock keys on loopback, and proves a RUM CLIENT span is the Rust
SERVER span's parent. No test data goes to shared Demo. Capture and screenshot are
under ignored `test-results/`. Without that bundle, the browser workflow still
passes but stored browser/server correlation remains a Corridora UI check.

Fastly traces use official `fastly-opentelemetry-exporter = 0.2.0`, pinned and
vendored with one transport patch: upstream drops a `send_async` pending request
and never checks HTTP status; this sample calls synchronous `send` and checks 2xx,
using the same SDK/protobuf-derived OTLP JSON encoding. [Patch provenance](docs/exporter-patch.md)
explains the exact difference. A successful POST does not prove stored correlation.
Native API provisioning, account quotas, live TLS/DNS, native log/stats delivery and
hosted daily execution require operator credentials and were not claimed by local
tests. The planned Fastly cost is $0 within current account allowances under
100,000 dynamic requests and 0.5 GB delivery/month; verify your account terms and
bill. No Object Storage, paid ads or paid AI is enabled.
