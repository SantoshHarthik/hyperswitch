# Adaptive Retry — Feature Spec

## What it does

When a payment fails on connector A, instead of returning an error to the merchant,
Hyperswitch automatically retries the payment on connector B (or C). The connector
chosen for the retry is not random — it is the one with the fewest recorded failures
in the last 10 minutes.

## Algorithm

1. A payment attempt fails and reaches the retry loop in `do_gsm_actions()`.
2. The GSM (Gateway Status Mapping) table returns `GsmDecision::Retry` for this
   connector + error-code combination.
3. `record_failure(connector_name)` is called, adding a timestamped entry to the
   in-memory sliding window for that connector.
4. The remaining connectors in the routing list are collected into a Vec.
5. `pick_best(candidates)` reads the tracker and returns the candidate with the
   fewest failures in the last 10 minutes. Ties are broken by position — the first
   connector in the routing list wins.
6. That connector is extracted from the list and used for the next attempt.

## Sliding window

- Window length: **10 minutes** (600 seconds), configured in `connector_health`.
- Storage: `HashMap<String, Vec<Instant>>` — one entry per connector, holding the
  wall-clock timestamp of every failure.
- Pruning: stale entries are dropped on every `record_failure` call and on every
  `failure_count` read. No background job required.
- Concurrency: the global tracker is wrapped in `tokio::sync::RwLock`. Reads
  (`pick_best`) take a shared lock; writes (`record_failure`) take an exclusive lock.

## Failure selection (not a threshold)

The algorithm is **minimum-selection**, not threshold-based. There is no threshold
count that blocks a connector — a connector with 10 failures is still used if all
other candidates have 11 or more. The intent is to shift traffic toward healthier
connectors gradually, not to hard-block a failing one.

## Known limitation

Only failures that reach the retry path are recorded. If a connector fails on first
attempt and GSM says `DoDefault` (not `Retry`), that failure is not tracked. So
connectors that consistently fail in non-retryable ways will not be deprioritized.

## Where the code lives

| File | Purpose |
|------|---------|
| `crates/connector_health/src/lib.rs` | Core tracker struct — pure std, no dependencies |
| `crates/connector_health/Cargo.toml` | Zero-dependency micro-crate |
| `crates/router/src/core/payments/connector_health.rs` | Async global wrapper (LazyLock + RwLock) |
| `crates/router/src/core/payments/retry.rs` | Integration point — calls record_failure and pick_best |
| `crates/router/src/core/payments.rs` | Module declaration for connector_health |
| `crates/router/Cargo.toml` | Added connector_health dependency |

## Running the tests

```bash
cargo test -p connector_health -- --nocapture
```

All 9 tests run in under 1 second with no database, no Redis, and no system
dependencies beyond Rust itself. The `test_demo_three_phase_walkthrough` test
prints a formatted walkthrough of the three phases (healthy → degraded → recovered)
suitable for a live demo.
