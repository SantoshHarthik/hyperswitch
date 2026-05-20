use std::sync::LazyLock;

use connector_health::ConnectorHealthTracker;
use router_env::logger;
use tokio::sync::RwLock;

static CONNECTOR_HEALTH: LazyLock<RwLock<ConnectorHealthTracker>> =
    LazyLock::new(|| RwLock::new(ConnectorHealthTracker::default_window()));

/// Records a failure for the given connector in the global 10-minute window tracker.
pub async fn record_failure(connector: &str) {
    let mut tracker = CONNECTOR_HEALTH.write().await;
    tracker.record_failure(connector);
    logger::debug!(
        connector = %connector,
        "connector_health: failure recorded in sliding window"
    );
}

/// Returns the best connector to retry on from the candidates,
/// based on fewest failures recorded in the last 10 minutes.
pub async fn pick_best(candidates: &[&str]) -> Option<String> {
    let tracker = CONNECTOR_HEALTH.read().await;
    tracker.pick_best_connector(candidates).map(str::to_string)
}
