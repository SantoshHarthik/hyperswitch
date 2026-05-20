# Hyperswitch — Adaptive Retry Fork

> This is a fork of [Hyperswitch](https://github.com/juspay/hyperswitch) with one
> feature added: **health-aware adaptive retry**. When a payment fails and GSM says
> to retry, the router picks the connector with the fewest failures in the last
> 10 minutes instead of the next connector in a static list.

---

## What this fork adds

- **`crates/connector_health/`** — a zero-dependency micro-crate that tracks
  per-connector failure counts in a sliding 10-minute window using only `std::`.
- **`crates/router/src/core/payments/connector_health.rs`** — async global wrapper
  (a `LazyLock<RwLock<ConnectorHealthTracker>>`) that exposes `record_failure` and
  `pick_best` to the retry loop.
- **`crates/router/src/core/payments/retry.rs`** — two lines added to the
  `GsmDecision::Retry` arm: record the failure, then use `pick_best` to choose the
  next connector.

The original Hyperswitch README is preserved at [README_ORIGINAL.md](./README_ORIGINAL.md).

---

## Run the demo in under 10 minutes

### Prerequisites

- Rust toolchain (`rustup` installed)
- Nothing else — the demo runs with zero system dependencies

### Clone and run

```bash
git clone https://github.com/SantoshHarthik/hyperswitch.git
cd hyperswitch
cargo test -p connector_health -- --nocapture
```

That's it. No database. No Redis. No Docker.

---

## What you should see

```
running 9 tests

╔══════════════════════════════════════════════════════════╗
║         ADAPTIVE RETRY — CONNECTOR HEALTH DEMO          ║
╚══════════════════════════════════════════════════════════╝

── Phase 1: Normal operation (no failures recorded) ─────────
   stripe: 0  adyen: 0  checkout: 0
   → Routing to: Some("stripe")  (tie at zero — first in list wins)

── Phase 2: Stripe starts failing — adaptive routing kicks in ─
   stripe: 3  adyen: 0  checkout: 0
   → Adaptive retry chose: Some("adyen")  (routed away from stripe)

── Phase 3: 10-min window expires — stripe healthy again ─────
   stripe: 0  adyen: 0  checkout: 0
   → Routing to: Some("stripe")  (all failures expired)

test result: ok. 9 passed; 0 failed; finished in 0.35s
```

This single command is the canonical demo for this submission. It requires no
database, no Docker, and no external setup.

The three phases show: normal routing → adaptive routing away from a failing
connector → automatic recovery once the window expires.

---

## Sending a test payment

A Postman collection is included at [`postman/`](./postman/). Import it into
Postman to send payments through this fork against a running server.

When the server is running, adaptive retry log lines will fire in server output
during retries with the format:

```
adaptive_retry: picked connector with fewest failures in 10-min window
```

The `cargo test` command above is the main demo for this submission since it needs
no database or server. Postman is for full end-to-end testing once the stack is up.

---

## Core files

| File | What it does |
|------|-------------|
| `crates/connector_health/src/lib.rs` | Tracker struct + all 9 tests |
| `crates/router/src/core/payments/connector_health.rs` | Async global, wires tracker into router |
| `crates/router/src/core/payments/retry.rs` | Where `record_failure` and `pick_best` are called |
| `SPEC.md` | Algorithm, sliding window design, known limitations |
| `DECISIONS.md` | Why this approach, what's hacky, what comes next |
