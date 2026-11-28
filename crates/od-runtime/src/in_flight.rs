//! Spaces `tool_in_flight` updates of one tool call in time.
//!
//! The parser sends one per kilobyte of arguments; a fast model, or lines
//! that arrive in a burst after a stall, could still send many per second.
//! The first one of a call always goes out (it names the file); later ones
//! wait at least [`IN_FLIGHT_MIN_INTERVAL`], so at most four a second.

use std::collections::HashMap;
use std::time::Duration;
use tokio::time::Instant;

/// Shortest time between two updates of the same call.
pub const IN_FLIGHT_MIN_INTERVAL: Duration = Duration::from_millis(250);

/// When each call's last update went out.
#[derive(Debug, Default)]
pub struct InFlightGate {
    last: HashMap<String, Instant>,
}

impl InFlightGate {
    /// Whether the update of call `id` at `now` goes out; records it if so.
    pub fn admit(&mut self, id: &str, now: Instant) -> bool {
        match self.last.get_mut(id) {
            Some(at) if now.duration_since(*at) < IN_FLIGHT_MIN_INTERVAL => false,
            Some(at) => {
                *at = now;
                true
            }
            None => {
                self.last.insert(id.to_owned(), now);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_update_of_a_call_always_goes_then_four_a_second_at_most() {
        let t0 = Instant::now();
        let mut gate = InFlightGate::default();
        let at = |ms| t0 + Duration::from_millis(ms);
        // Updates every 50 ms for two seconds.
        let sent: Vec<u64> = (0..40)
            .map(|i| i * 50)
            .filter(|&ms| gate.admit("a", at(ms)))
            .collect();
        assert_eq!(sent, [0, 250, 500, 750, 1000, 1250, 1500, 1750]);
    }

    #[test]
    fn calls_are_spaced_on_their_own() {
        let t0 = Instant::now();
        let mut gate = InFlightGate::default();
        assert!(gate.admit("a", t0));
        assert!(gate.admit("b", t0 + Duration::from_millis(10)));
        assert!(!gate.admit("a", t0 + Duration::from_millis(20)));
        assert!(!gate.admit("b", t0 + Duration::from_millis(259)));
        assert!(gate.admit("b", t0 + Duration::from_millis(260)));
    }
}
