# Full Conversation Transcript — Adaptive Retry Feature Build
Session: 8f4644ca-b987-4c1f-84e2-36c65f82a79e | Date: 2026-05-20

---

## USER
can u explain me in plain english: 1. what hyperswitch does 2. what is payment connector 3. what does routing mean in this context

## ASSISTANT
**1. What Hyperswitch does**

Hyperswitch is an open-source payment orchestration platform — think of it as a smart middleman between your app and payment processors. Instead of your app talking directly to Stripe (or PayPal, or Adyen), it talks to Hyperswitch, which then handles talking to whichever payment processor makes sense. This gives you one unified API regardless of how many payment providers you're using behind the scenes.

**2. What a payment connector is**

A connector is an integration adapter for a specific payment processor — Stripe, Braintree, Adyen, Checkout.com, etc. Each connector translates between Hyperswitch's internal format and that processor's specific API format. So when you charge a card, Hyperswitch hands the request to the right connector, the connector reformats it into what, say, Stripe expects, sends it, gets a response back, and translates that back into Hyperswitch's standard format. There are ~50+ connectors in this repo.

**3. What routing means here**

Routing is the logic that decides which connector to send a payment through. Given a payment request, routing answers: "Should this go to Stripe or Adyen or Checkout.com?" The decision can be based on rules like:
- **Rule-based**: "If card is Visa and amount > $100, use Adyen"
- **Volume-based**: "Split 70% to Stripe, 30% to Braintree"
- **Fallback/retry**: "Try Stripe first; if it fails, try Checkout.com"
- **Cost-based**: route to whichever processor charges the lowest fee for this transaction type

So routing is essentially the traffic controller that maximizes payment success rates and minimizes costs across all your connected processors.

---

## USER
now explain the folder structure of this repo at a high level

## ASSISTANT
Here's a map of the repo:

**Top-level layout**
```
crates/          — all Rust source code (the actual application)
config/          — environment config files (dev, prod, docker, etc.)
migrations/      — database schema migrations (PostgreSQL via Diesel)
cypress-tests/   — end-to-end API tests
postman/         — Postman collections for manual API testing
docs/            — additional documentation
docker/          — Docker-related files
proto/           — gRPC/protobuf definitions
scripts/         — build/deploy helper scripts
```

**Inside `crates/` — the core code**

| Crate | What it does |
|---|---|
| `router` | The heart of the app — HTTP server, all API handlers, business logic for payments, refunds, disputes |
| `hyperswitch_connectors` | All connector integrations (Stripe, Adyen, Braintree, etc.) |
| `hyperswitch_interfaces` | Trait definitions every connector must implement |
| `hyperswitch_domain_models` | Core domain types: Payment, Refund, Mandate, etc. |
| `api_models` | Request/response types for the public-facing API |
| `diesel_models` | Database table structs — maps Rust types to DB rows |
| `storage_impl` | Database query logic (reads/writes using Diesel ORM) |
| `euclid` | The routing engine — rule evaluation and decision logic |
| `analytics` | Payment analytics and reporting queries |
| `redis_interface` | Redis client abstraction |
| `scheduler` | Background job scheduler |
| `common_enums` / `common_types` / `common_utils` | Shared primitives used across all other crates |
| `router_env` | Logging, tracing, environment setup |

---

## USER
i want to add any of these 5 routing features: 1. cost aware routing, 2. time of day failover, 3. bin based routing, 4. adaptive retry, 5. velocity based fraud gate. before proceeding can u explain me all of these with examples and when they could be used and audit the codebase and for each one tell me whether it already exists fully, exists partly, or is missing completely.

## ASSISTANT
**1. Cost-Aware Routing — PARTIALLY EXISTS**

Every payment processor charges you a fee. Cost-aware routing automatically picks the cheapest processor for each transaction. Example: Stripe charges 1.5%, Adyen charges 1.2% — route to Adyen. There's cost comparison logic for co-badged debit cards (choosing between Visa/Mastercard rails), but no general rule like "if processor fee > X%, use connector Y." The Euclid routing engine has no cost/fee condition type.

**2. Time-of-Day Failover — COMPLETELY MISSING**

Route payments differently based on time of day or day of week. Example: a Brazilian processor has maintenance windows 2am–4am UTC — route to Adyen during that window. Zero time-based routing logic exists anywhere. No time conditions in the routing rule engine.

**3. BIN-Based Routing — PARTIALLY EXISTS**

Route based on the first 6–8 digits of a card number (the BIN), which identifies the issuing bank, card type, and country. BIN lookup infrastructure exists, but there's no routing hook that uses it to make connector decisions.

**4. Adaptive Retry — PARTIALLY EXISTS**

When a payment fails on connector A, automatically retry on connector B. The retry loop, GSM table, `GsmDecision::Retry`, and `ConnectorCallType::Retryable` all exist. The gap: when GSM says Retry, the loop blindly picks the next connector in the static list with no health awareness.

**5. Velocity-Based Fraud Gate — COMPLETELY MISSING**

Block payments that look like fraud based on velocity (too many transactions in a short window). Requires a counter store (Redis or DB), a threshold config per merchant, and a blocking decision injected early in the payment flow. None of this exists.

---

## USER
can u choose between Adaptive retry and Cost-aware routing which is less complex and has less hidden complexities. i need to do it minimum time.

## ASSISTANT
**Recommendation: Adaptive Retry**

Cost-aware routing looks simpler but requires a fee table in the database — no demo is possible without that data seeded. The WASM complication: the Euclid routing engine compiles to WebAssembly so the rules UI works in the browser. Any new condition type must also work in the WASM build, a separate compilation target that can break in non-obvious ways.

Adaptive Retry has a massive advantage: the plumbing already exists. The retry loop, GSM failure handler, and `ConnectorCallType::Retryable` type are all built. The gap is just the selection logic — which connector to pick on retry. No WASM changes, no new database tables, change is contained to a handful of files.

---

## USER
can u explain me what adaptive retry is? how does it work, show me places in code where the partial implementation is done

## ASSISTANT
**What is Adaptive Retry?**

When a payment fails on connector A (say Stripe), the system automatically tries connector B (say Adyen) without the customer knowing. The "adaptive" part means the system is smart about when to retry and which connector to try next, based on the type of failure.

**The 3 actors:**

1. **GSM — Gateway Status Mapping** (`crates/router/src/core/gsm.rs`) — A lookup table of failure rules. Answers: "When connector X returns error code Y, what should I do?" Each row maps connector + error code → `Retry` or `DoDefault`.

2. **RetryFeatureData** (`crates/common_types/src/domain.rs`, line 170) — Holds the options for what to do on retry: `step_up_possible`, `clear_pan_possible`, `alternate_network_possible`, `decision: GsmDecision`.

3. **GsmDecision** (`crates/common_enums/src/enums.rs`, line 379) — Just two options: `Retry` or `DoDefault` (give up).

**The gap:** `do_gsm_actions()` in `retry.rs` has the retry loop. When `GsmDecision::Retry` fires, it picks the next connector from a static iterator — no health awareness, no intelligence about which connector is currently performing better.

---

## USER
i have 2 hrs to build this. give me the minimal changes needed. i need to: 1. send a test payment, 2. see custom logic fire in logs, 3. run tests with one command.

## ASSISTANT
After deep analysis: the existing code ALREADY retries on a different connector. The retry loop at `retry.rs:200` already picks the next connector when GSM says `Retry`. What's missing:

1. No log line showing which connector was chosen and why
2. No intelligence about which connector to pick (just next-in-list)
3. No tests for the selection logic

**Minimal changes: 2 files, no migration**

`RetryFeatureData` is stored as JSONB (`impl_to_sql_from_sql_json!` at domain.rs:182). Adding an optional field means existing rows get `None` — no DB column, no migration needed.

---

## USER
i must be able to demo without a database. i dont have sql on my computer.

## ASSISTANT
**The plan:** `RetryFeatureData` is JSONB, no migration. `common_types` crate has zero DB dependencies — tests there run with just `cargo test`.

Demo plan:
- Run `cargo test -p common_types -- --nocapture`
- Output shows the adaptive retry decision firing with connector names
- No database, no server, no curl needed

---

## USER
[Request interrupted — user redirected]

hold on. i read in an article that the in memory tracker approach is the standard way to do adaptive retry.

## ASSISTANT
**In-Memory Tracker vs GSM Rule-Based:**

| | In-Memory Tracker | GSM Rule-Based |
|---|---|---|
| Needs DB | No | No (JSONB trick) |
| Needs config/rules setup | No — just works | Yes — must create GSM rule first |
| Demo story | "It learns automatically" | "I configured a fallback rule" |
| Files changed | 1 (retry.rs) | 2 |

In-memory tracker is simpler AND makes a better demo story. "The system adapts automatically as connectors fail" is more impressive than "I manually told it to use Adyen."

The log line for your demo:
```
adaptive_retry: stripe failed (failure_count=2), picking lowest-failure connector: adyen (failure_count=0)
```

---

## USER
yes lets go with the in memory tracker but i want it in its own dedicated file. keeps things clean and easy to test. also use a 10 minute sliding window with timestamps, not a simple counter.

## ASSISTANT
Good call. Let me read the codebase structure before writing anything.

---

## USER
hold on. two things: 1. name the file connector_health.rs — there's already a currency.rs that does similar stuff with a hashmap. 2. read currency.rs first and copy its pattern exactly. for tests i want 6–8 tests plus one special demo walkthrough test showing 3 phases. pls dont make changes yet.

## ASSISTANT
Read `currency.rs`. Pattern it uses:
- `std::sync::LazyLock` (not `OnceLock`) for the static
- `tokio::sync::RwLock` (not `std::sync::Mutex`) — fits the async codebase
- Public module-level async functions as the API, not direct static access
- `logger::debug!` for operational messages
- Inner struct holds `HashMap<String, Vec<...>>`

**Plan:**
```
static CONNECTOR_HEALTH: LazyLock<RwLock<ConnectorHealthTracker>>

ConnectorHealthTracker {
    failures: HashMap<String, Vec<Instant>>
    window: Duration
}
```

9 tests: fresh tracker, record+count, pick fewest failures, tie by list order, single candidate, empty candidates, sliding window expiry, partial expiry, demo walkthrough (3 phases using 120ms window).

Changes: 3 files — new `connector_health.rs`, add `pub mod connector_health;` to `payments.rs`, call `record_failure` + `pick_best` in `retry.rs`.

---

## USER
okk make the changes

## ASSISTANT
Implementation complete. During implementation, discovered that `cargo test -p router` fails because `protoc` (gRPC) is not installed, and `cargo test -p common_types` fails because `libpq` (postgres) is not installed. Nearly ALL crates in the workspace transitively depend on postgres or gRPC.

**Solution: Created `crates/connector_health/` as a zero-dependency micro-crate** with only `std::`. `members = ["crates/*"]` glob auto-includes it.

**Files created/modified:**

| File | Action |
|------|--------|
| `crates/connector_health/Cargo.toml` | Created — zero dependencies |
| `crates/connector_health/src/lib.rs` | Created — tracker struct + 9 tests |
| `crates/router/src/core/payments/connector_health.rs` | Created — LazyLock+RwLock async globals |
| `crates/router/src/core/payments/retry.rs` | Modified — record_failure + pick_best in retry loop |
| `crates/router/src/core/payments.rs` | Modified — added `pub mod connector_health;` |
| `crates/router/Cargo.toml` | Modified — added connector_health dependency |

**Result:** `cargo test -p connector_health -- --nocapture` → 9 tests pass in 0.14s. No DB, no Docker, no system deps beyond Rust.

---

## USER
tests are passing. now i need the docs for submission. create these files one at a time and show me each before committing:
1. SPEC.md at repo root
2. README.md at repo root (keep original as README_ORIGINAL.md)
3. DECISIONS.md at repo root

---

## SPEC.md — review and fixes

**User fix 1:** "failure selection" section had sentence starting with "count that blocks a connector" — missing subject. Fixed to "There is no threshold count that blocks a connector."

**User fix 2:** Known limitation rewritten to: "Only failures that reach the retry path are recorded. If a connector fails on first attempt and GSM says DoDefault (not Retry), that failure is not tracked. So connectors that consistently fail in non-retryable ways will not be deprioritized."

**Written to:** `/SPEC.md`

---

## README.md — review and fixes

**User fix 1:** Replaced `<your-fork-url>` with `https://github.com/SantoshHarthik/hyperswitch.git`

**User fix 2:** Added after output block: "This single command is the canonical demo for this submission. It requires no database, no Docker, and no external setup."

**User addition:** Added "Sending a test payment" section — confirmed `postman/` folder exists, points to it, notes adaptive retry log format: `adaptive_retry: picked connector with fewest failures in 10-min window`, notes cargo test is main demo and Postman is for full E2E.

**Written to:** `/README.md` (original preserved as `/README_ORIGINAL.md`)

---

## DECISIONS.md — review and fixes

**User fix 1:** "Why a separate crate" — sentence rewritten to: "Every other crate in the workspace transitively depends on PostgreSQL (via `common_enums` and `diesel`) or gRPC (via `unified-connector-service-client`). A standalone crate was the only way to make the tests runnable without system dependencies."

**User fix 2:** Third bullet in "Why in-memory over GSM fallback" replaced from "Adding even an optional JSONB field risks confusing the existing GSM rule engine" to: "GSM fallback would require merchant configuration which is more complex than a pure runtime observation layer."

**Written to:** `/DECISIONS.md`

---

## Final state

All docs written. Commit pending.

**Demo command:**
```bash
cargo test -p connector_health -- --nocapture
```

No database. No Docker. No system dependencies beyond Rust.
