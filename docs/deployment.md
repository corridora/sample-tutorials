# Fastly deployment and operating budget

Published 2026-10-11 · Updated 2026-10-11

Install an official Rust toolchain with `wasm32-wasip1`, Node 24+, Python 3.11+,
and optionally the GitHub CLI for scheduled browser journeys. A Fastly account
with enabled Compute/KV/Secret Store and sufficient resource quotas is needed.
The native Homebrew compiler may not accept official prebuilt target libraries;
use an official rustup toolchain rather than mixing distributions.

```sh
rustup toolchain install 1.86.0 --target wasm32-wasip1
# Ensure cargo/rustc default to that official toolchain.
./setup configure
./setup deploy --plan
./setup deploy --apply
./setup run
./setup verify
```

Cloud setup performs the API operations implemented in `scripts/setup_app.py`:
customer verification, dedicated service creation or explicit reuse, active-version
clone, domain, named KV/Config/Secret Store creation/linking, seeded catalog,
secret updates, exact TLS origin/export backends, optional HTTPS logging endpoint,
Wasm package upload, validation and activation. Records in `.sample/resources.json`
are saved after each resource creation. Keep this inventory to resume or tear down.
The deployment uses API requests rather than printing shell commands. The script
never creates a paid Object Storage instance or purchases a custom domain.

Set distinct store names per independently deployed sample. If a store name
already exists, setup fails with an explicit ID-reuse instruction instead of
assuming ownership. Existing store IDs require explicit operator selection and
are never deleted. A service's selected Config/Secret stores are versionless:
updates affect every version using those stores. Use dedicated stores when other
service versions should remain isolated.

The default provider hostname is `<FASTLY_RESOURCE_PREFIX>.edgecompute.app`.
Availability and acceptance must be confirmed by Fastly. For a custom domain,
first create the domain/TLS subscription in Fastly, add the displayed `_acme-challenge`
DNS CNAME, wait for certificate issuance, then point the requested hostname at the
Fastly DNS target shown for the TLS configuration. Supply that exact HTTPS
`PUBLIC_URL`. Setup attaches the domain to the service but does not automate DNS
or TLS subscriptions because they can be shared with other services.

Cloud origin regression requires a controlled HTTPS endpoint that supports
`/api/fixture?slow=1`, gives a bounded 1.5-second response delay and marks healthy
fixtures publicly cacheable. The Akamai sample offers it. Alternatively, host
`scripts/fixture_origin.py` behind your existing TLS proxy; its loopback port is
4206. The local Rust runtime supplies that contract itself. Public third-party
slow endpoints are not used. Keep origin cache/trace/auth policies explicit.

The Fastly plan assumes $0 incremental spend within your current recurring
allowances, at most 100,000 small dynamic app requests, <0.5 GB delivery, tiny
store data and bounded telemetry/runner traffic each month. Export requests and
KV/config/secret operations count too. Account terms/legacy trials differ; check
[Fastly pricing](https://www.fastly.com/pricing) and
[account types](https://www.fastly.com/documentation/guides/organizations-and-accounts/billing/account-types/).
A new adapter/origin host is additional spend; reuse the fleet's Akamai VM or
existing infrastructure to maintain the fleet's <$20 monthly plan. No budget
alert is a billing cap. Disable scheduled synthetics/provider forwarding first
if the fleet approaches its reserve, and inspect usage weekly. Static content,
diagnostics and API responses are intentionally simple; this sample does not
claim rate limiting protects an unbounded public traffic bill.

No live account resources, provider quota verification, stored Demo ingestion,
custom DNS/TLS challenges or hosted daily evidence are established by local
build/tests. Those are explicit post-deployment checks.

Official references for the implemented APIs:

- [Fastly Rust SDK](https://www.fastly.com/documentation/reference/compute/sdks/rust/fastly/)
- [KV Store API](https://www.fastly.com/documentation/reference/api/services/resources/kv-store/)
- [Config Store items](https://www.fastly.com/documentation/reference/api/services/resources/config-store-item/)
- [Secret Store secrets](https://www.fastly.com/documentation/reference/api/services/resources/secret-store-secret/)
- [Compute manifest](https://www.fastly.com/documentation/reference/compute/fastly-toml/)
- [Cache behavior](https://www.fastly.com/documentation/guides/concepts/cache/)
