use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

const WINDOW: Duration = Duration::from_secs(600); // 10-minute sliding window

/// Tracks per-connector failure timestamps within a sliding time window.
/// Used by adaptive retry to prefer connectors with fewer recent failures.
#[derive(Debug)]
pub struct ConnectorHealthTracker {
    failures: HashMap<String, Vec<Instant>>,
    window: Duration,
}

impl ConnectorHealthTracker {
    /// Creates a tracker with the given sliding window duration.
    pub fn new(window: Duration) -> Self {
        Self {
            failures: HashMap::new(),
            window,
        }
    }

    /// Creates a tracker with the default 10-minute window.
    pub fn default_window() -> Self {
        Self::new(WINDOW)
    }

    /// Records a failure for a connector and prunes entries outside the window.
    pub fn record_failure(&mut self, connector: &str) {
        let entry = self.failures.entry(connector.to_string()).or_default();
        entry.push(Instant::now());
        let window = self.window;
        entry.retain(|t| t.elapsed() < window);
    }

    /// Returns how many failures occurred within the sliding window.
    pub fn failure_count(&self, connector: &str) -> usize {
        self.failures
            .get(connector)
            .map(|times| times.iter().filter(|t| t.elapsed() < self.window).count())
            .unwrap_or(0)
    }

    /// Returns the candidate with the fewest failures in the window.
    /// Ties are broken by position — first in the list wins.
    pub fn pick_best_connector<'a>(&self, candidates: &[&'a str]) -> Option<&'a str> {
        candidates
            .iter()
            .min_by_key(|&&c| self.failure_count(c))
            .copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_fresh_tracker_has_zero_failures() {
        let tracker = ConnectorHealthTracker::new(Duration::from_secs(600));
        assert_eq!(tracker.failure_count("stripe"), 0);
        assert_eq!(tracker.failure_count("adyen"), 0);
    }

    #[test]
    fn test_records_failure_and_counts_it() {
        let mut tracker = ConnectorHealthTracker::new(Duration::from_secs(600));
        tracker.record_failure("stripe");
        tracker.record_failure("stripe");
        assert_eq!(tracker.failure_count("stripe"), 2);
        assert_eq!(tracker.failure_count("adyen"), 0);
    }

    #[test]
    fn test_picks_connector_with_fewest_failures() {
        let mut tracker = ConnectorHealthTracker::new(Duration::from_secs(600));
        tracker.record_failure("stripe");
        tracker.record_failure("stripe");
        tracker.record_failure("adyen");
        // stripe: 2, adyen: 1, checkout: 0

        let best = tracker.pick_best_connector(&["stripe", "adyen", "checkout"]);
        assert_eq!(best, Some("checkout"));
    }

    #[test]
    fn test_tie_broken_by_list_order() {
        let tracker = ConnectorHealthTracker::new(Duration::from_secs(600));
        // all at 0 failures — first in list wins
        let best = tracker.pick_best_connector(&["adyen", "stripe", "checkout"]);
        assert_eq!(best, Some("adyen"));
    }

    #[test]
    fn test_single_candidate_always_returned() {
        let mut tracker = ConnectorHealthTracker::new(Duration::from_secs(600));
        tracker.record_failure("stripe");
        tracker.record_failure("stripe");
        tracker.record_failure("stripe");

        let best = tracker.pick_best_connector(&["stripe"]);
        assert_eq!(best, Some("stripe"));
    }

    #[test]
    fn test_empty_candidates_returns_none() {
        let tracker = ConnectorHealthTracker::new(Duration::from_secs(600));
        assert_eq!(tracker.pick_best_connector(&[]), None);
    }

    #[test]
    fn test_sliding_window_expires_old_failures() {
        let mut tracker = ConnectorHealthTracker::new(Duration::from_millis(60));
        tracker.record_failure("stripe");
        tracker.record_failure("stripe");

        // within window: stripe=2, adyen=0 → picks adyen
        assert_eq!(
            tracker.pick_best_connector(&["stripe", "adyen"]),
            Some("adyen")
        );

        std::thread::sleep(Duration::from_millis(80)); // past the 60ms window

        // failures expired: both at 0, tie broken by list order → stripe wins
        assert_eq!(tracker.failure_count("stripe"), 0);
        assert_eq!(
            tracker.pick_best_connector(&["stripe", "adyen"]),
            Some("stripe")
        );
    }

    #[test]
    fn test_partial_expiry_keeps_recent_failures() {
        let mut tracker = ConnectorHealthTracker::new(Duration::from_millis(80));
        tracker.record_failure("stripe");
        tracker.record_failure("stripe");

        std::thread::sleep(Duration::from_millis(50)); // half the window

        tracker.record_failure("stripe"); // this one is fresh

        std::thread::sleep(Duration::from_millis(50)); // first two expired, third still valid

        assert_eq!(tracker.failure_count("stripe"), 1);
    }

    #[test]
    fn test_demo_three_phase_walkthrough() {
        println!();
        println!("╔══════════════════════════════════════════════════════════╗");
        println!("║         ADAPTIVE RETRY — CONNECTOR HEALTH DEMO          ║");
        println!("╚══════════════════════════════════════════════════════════╝");

        let mut tracker = ConnectorHealthTracker::new(Duration::from_millis(120));
        let candidates = ["stripe", "adyen", "checkout"];

        // ── Phase 1: Clean slate, all connectors healthy ──────────────────
        println!();
        println!("── Phase 1: Normal operation (no failures recorded) ─────────");
        println!(
            "   stripe: {}  adyen: {}  checkout: {}",
            tracker.failure_count("stripe"),
            tracker.failure_count("adyen"),
            tracker.failure_count("checkout"),
        );
        let pick = tracker.pick_best_connector(&candidates);
        println!("   → Routing to: {:?}  (tie at zero — first in list wins)", pick);
        assert_eq!(pick, Some("stripe"));

        // ── Phase 2: Stripe accumulates failures, routing shifts ──────────
        println!();
        println!("── Phase 2: Stripe starts failing — adaptive routing kicks in ─");
        tracker.record_failure("stripe");
        tracker.record_failure("stripe");
        tracker.record_failure("stripe");
        println!(
            "   stripe: {}  adyen: {}  checkout: {}",
            tracker.failure_count("stripe"),
            tracker.failure_count("adyen"),
            tracker.failure_count("checkout"),
        );
        let pick = tracker.pick_best_connector(&candidates);
        println!("   → Adaptive retry chose: {:?}  (routed away from stripe)", pick);
        assert_eq!(pick, Some("adyen"));

        // ── Phase 3: Window expires, stripe recovers ──────────────────────
        println!();
        println!("── Phase 3: 10-min window expires — stripe healthy again ─────");
        std::thread::sleep(Duration::from_millis(140)); // past the 120ms test window
        println!(
            "   stripe: {}  adyen: {}  checkout: {}",
            tracker.failure_count("stripe"),
            tracker.failure_count("adyen"),
            tracker.failure_count("checkout"),
        );
        let pick = tracker.pick_best_connector(&candidates);
        println!("   → Routing to: {:?}  (all failures expired)", pick);
        assert_eq!(tracker.failure_count("stripe"), 0);
        assert_eq!(pick, Some("stripe"));

        println!();
    }
}
