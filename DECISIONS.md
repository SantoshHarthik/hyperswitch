# Decisions

## Why adaptive retry over the other 4 options

**Cost-aware routing** requires a cost table per connector per BIN range, stored in
the database. No demo is possible without that data seeded and a running DB.

**Time-of-day failover** requires a schedule config per connector and a cron-like
evaluation at request time. More config surface, less visible behavior in a 2-minute
demo.

**BIN-based routing** requires a BIN lookup table and real card numbers to route
against. Not demoable without a database and test card corpus.

**Velocity-based fraud gate** requires a counter store (Redis or DB), a threshold
config per merchant, and a blocking decision injected early in the payment flow.
More moving parts than any other option on this list.

**Adaptive retry** was chosen because the partial infrastructure (GSM table,
`GsmDecision::Retry`, `ConnectorCallType::Retryable`) already existed. The missing
piece was the selection logic — which connector to pick on retry. That one gap could
be filled with pure in-memory code and demoed with a single test command.

## Why in-memory tracker over GSM fallback

The GSM fallback approach would extend the existing `RetryFeatureData` struct with
a failure count field and persist it to the database as JSONB. It would survive
restarts and work across multiple router instances.

It was ruled out because:
- No local database available during development
- Every read/write would be an async DB round-trip in the hot path
- GSM fallback would require merchant configuration which is more complex than a
  pure runtime observation layer

The in-memory tracker is the right call for a single-instance demo. It requires no
infrastructure, has microsecond read latency, and the entire implementation fits in
one file with no dependencies.

## Why a separate crate

The tracker uses only `std::collections::HashMap` and `std::time::Instant`. Putting
it in `connector_health/` means `cargo test -p connector_health` runs in under a
second on any machine with Rust installed — no libpq, no protoc, no Docker.

Every other crate in the workspace transitively depends on PostgreSQL (via
`common_enums` and `diesel`) or gRPC (via `unified-connector-service-client`). A
standalone crate was the only way to make the tests runnable without system
dependencies.

## What's hacky right now

**Global mutable state.** `CONNECTOR_HEALTH` is a process-wide singleton. In a
multi-instance deployment, each router pod has its own independent view of connector
health. There is no cross-pod aggregation.

**First attempt not tracked.** `record_failure` is only called inside the retry
loop. A connector that fails 100% of first attempts and triggers `DoDefault` (not
`Retry`) will never accumulate failures in the tracker and will never be
deprioritized. See `SPEC.md` — Known limitation.

**No observability.** The only signal that adaptive routing fired is a
`logger::info!` line in `retry.rs`. There is no metric, no counter, and no endpoint
to inspect the current failure counts.

## What I would do with 4 more hours

**Redis-backed tracker.** Replace `HashMap<String, Vec<Instant>>` with a Redis
sorted set (score = timestamp, member = failure ID). All router pods share the same
view. Expiry is handled natively by Redis TTL. The `ConnectorHealthTracker` API
stays the same — only the storage layer changes.

**Routing trace endpoint.** Add a `GET /routing/health` endpoint that returns the
current failure count and window age for every tracked connector. Operators can see
in real time which connectors are accumulating failures without digging through logs.

**Prometheus metrics.** Emit a `connector_retry_failures_total` counter and a
`connector_health_window_failures` gauge per connector. Wire them into the existing
`router_env` metrics setup. This makes adaptive routing visible in any Grafana
dashboard without a custom endpoint.

## PII handling

Only connector names (e.g. `"stripe"`, `"adyen"`) are recorded in the tracker and
logged. No card numbers, no CVVs, no customer identifiers, no amounts, and no BIN
data are stored or logged anywhere in this feature.
