//! Auto-Danmu tracking and decision logic.
//!
//! Tracks incoming danmu messages per lesson and decides whether to send an
//! automatic reply based on frequency thresholds and cooldown periods.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// Tracks danmu messages to determine if an auto-reply should be triggered.
pub struct DanmuTracker {
    /// Maps danmu text content to a queue of timestamps when it was seen.
    /// Only tracks events within the current window.
    history: HashMap<String, VecDeque<Instant>>,
    /// Maps danmu text content to the timestamp of the last time we auto-replied to it.
    last_sent: HashMap<String, Instant>,
}

impl DanmuTracker {
    pub fn new() -> Self {
        Self {
            history: HashMap::new(),
            last_sent: HashMap::new(),
        }
    }

    /// Process an incoming danmu message and decide if an auto-reply is needed.
    ///
    /// # Arguments
    /// * `content` - The text of the incoming danmu
    /// * `threshold` - The number of occurrences required within the window to trigger a reply
    /// * `window_secs` - The time window in seconds to consider for the threshold
    /// * `cooldown_secs` - The minimum time in seconds before replying to the same content again
    ///
    /// # Returns
    /// `true` if an auto-reply should be sent, `false` otherwise.
    pub fn track_and_decide(
        &mut self,
        content: &str,
        threshold: usize,
        window_secs: u64,
        cooldown_secs: u64,
    ) -> bool {
        let content = content.trim().to_string();
        if content.is_empty() {
            return false;
        }

        let now = Instant::now();
        let window = Duration::from_secs(window_secs);
        let cooldown = Duration::from_secs(cooldown_secs);

        // Check if we are still in cooldown for this specific content
        if let Some(last) = self.last_sent.get(&content)
            && now.duration_since(*last) < cooldown
        {
            return false;
        }

        // Update history for this content
        let queue = self.history.entry(content.clone()).or_default();
        queue.push_back(now);

        // Clean up old entries outside the time window
        while let Some(&oldest) = queue.front() {
            if now.duration_since(oldest) > window {
                queue.pop_front();
            } else {
                break;
            }
        }

        // If the threshold is reached, trigger reply and update cooldown
        if queue.len() >= threshold {
            self.last_sent.insert(content, now);
            // Clear the queue so we don't spam if a burst continues right after cooldown
            queue.clear();
            true
        } else {
            false
        }
    }

    /// Periodic cleanup of internal state to prevent unbounded memory growth.
    /// Should be called occasionally, e.g., on a timer or between lessons.
    pub fn cleanup(&mut self, window_secs: u64, cooldown_secs: u64) {
        let now = Instant::now();
        let window = Duration::from_secs(window_secs);
        let cooldown = Duration::from_secs(cooldown_secs);

        // Remove old history queues
        self.history.retain(|_, queue| {
            while let Some(&oldest) = queue.front() {
                if now.duration_since(oldest) > window {
                    queue.pop_front();
                } else {
                    break;
                }
            }
            !queue.is_empty()
        });

        // Remove old cooldowns
        self.last_sent
            .retain(|_, &mut last| now.duration_since(last) < cooldown);
    }
}

impl Default for DanmuTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_when_threshold_met() {
        let mut tracker = DanmuTracker::new();
        assert!(!tracker.track_and_decide("1", 3, 60, 60)); // 1st
        assert!(!tracker.track_and_decide("1", 3, 60, 60)); // 2nd
        assert!(tracker.track_and_decide("1", 3, 60, 60)); // 3rd -> trigger!
    }

    #[test]
    fn do_not_trigger_when_different_messages() {
        let mut tracker = DanmuTracker::new();
        assert!(!tracker.track_and_decide("1", 3, 60, 60));
        assert!(!tracker.track_and_decide("2", 3, 60, 60));
        assert!(!tracker.track_and_decide("3", 3, 60, 60));
    }

    #[test]
    fn respect_cooldown() {
        let mut tracker = DanmuTracker::new();
        assert!(!tracker.track_and_decide("A", 2, 60, 60));
        assert!(tracker.track_and_decide("A", 2, 60, 60)); // triggers

        // Send 2 more immediately, should be blocked by cooldown
        assert!(!tracker.track_and_decide("A", 2, 60, 60));
        assert!(!tracker.track_and_decide("A", 2, 60, 60));
    }

    #[test]
    fn empty_content_never_triggers() {
        let mut tracker = DanmuTracker::new();
        assert!(!tracker.track_and_decide("", 1, 60, 60));
        assert!(!tracker.track_and_decide("   ", 1, 60, 60));
        assert!(!tracker.track_and_decide("\t\n", 1, 60, 60));
    }

    #[test]
    fn threshold_one_triggers_immediately() {
        let mut tracker = DanmuTracker::new();
        assert!(tracker.track_and_decide("hello", 1, 60, 60));
    }

    #[test]
    fn different_content_independent() {
        let mut tracker = DanmuTracker::new();
        // Track "A" twice out of 3 threshold
        assert!(!tracker.track_and_decide("A", 3, 60, 60));
        assert!(!tracker.track_and_decide("A", 3, 60, 60));
        // "B" shouldn't benefit from "A"'s count
        assert!(!tracker.track_and_decide("B", 3, 60, 60));
        // "A" reaches threshold
        assert!(tracker.track_and_decide("A", 3, 60, 60));
        // "B" still only at 1
        assert!(!tracker.track_and_decide("B", 3, 60, 60));
    }

    #[test]
    fn cleanup_removes_stale_entries() {
        let mut tracker = DanmuTracker::new();
        // Track some content
        tracker.track_and_decide("X", 10, 60, 60);
        tracker.track_and_decide("Y", 10, 60, 60);

        // Manual insertion of old timestamps isn't easy with Instant::now()
        // but we can test that it doesn't remove fresh ones
        tracker.cleanup(60, 60);
        assert!(!tracker.history.is_empty());

        // Test cooldown cleanup
        tracker.track_and_decide("Z", 1, 60, 60); // triggers
        assert!(tracker.last_sent.contains_key("Z"));
        tracker.cleanup(60, 60);
        assert!(tracker.last_sent.contains_key("Z")); // still in cooldown
    }

    #[test]
    fn window_expiration_prevents_trigger() {
        let mut tracker = DanmuTracker::new();
        // We can't easily mock time for Instant, but we can test the logic
        // if we had a way to control time. Since we don't, we'll just add
        // a test that exercise the cleanup path.
        tracker.track_and_decide("A", 2, 0, 60); // 0s window
        assert!(!tracker.track_and_decide("A", 2, 0, 60)); // should have expired immediately
    }

    #[test]
    fn large_history_cooldown_cleanup() {
        let mut tracker = DanmuTracker::new();
        for i in 0..100 {
            tracker.track_and_decide(&i.to_string(), 1, 60, 0); // trigger immediately, 0s cooldown
        }
        assert_eq!(tracker.last_sent.len(), 100);
        tracker.cleanup(60, 0); // all cooldowns expired
        assert!(tracker.last_sent.is_empty());
    }
}
