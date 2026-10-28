//! Shared AS2/AS3 timer identifiers and callback lifecycle.
//! The nonzero-ID and cancellation contracts are adapted from Ruffle's MIT
//! timer implementation. We retain the existing browser Timer scheduler;
//! this registry contains no browser references or independent clock.
//! See docs/RUNTIME-EASING-PROVENANCE.md for the pinned source and boundaries.

use std::collections::HashMap;

const MAX_ID: u32 = i32::MAX as u32;

pub struct ScriptTimerIds {
    active: HashMap<u32, bool>,
    next_id: u32,
    limit: usize,
}

impl ScriptTimerIds {
    pub fn new(limit: usize) -> Result<Self, String> {
        if !(1..=10_000).contains(&limit) {
            return Err("Timer registry limit must be from 1 through 10000".into());
        }
        Ok(Self {
            active: HashMap::new(),
            next_id: 1,
            limit,
        })
    }

    /// A positive identifier is shared by both dialects and both clear APIs.
    pub fn register(&mut self, delay: f64, repeating: bool) -> Result<u32, String> {
        if !delay.is_finite() || !(0.0..=MAX_ID as f64).contains(&delay) {
            return Err(
                "Timer delay requires a finite number from 0 through 2147483647 milliseconds"
                    .into(),
            );
        }
        if self.active.len() >= self.limit {
            return Err(format!(
                "This player supports at most {} active callback timers",
                self.limit
            ));
        }
        // IDs never reset at a publication boundary: a retained stale ID must
        // not cancel a newly created callback after Restart.
        while self.active.contains_key(&self.next_id) {
            self.advance_id();
        }
        let id = self.next_id;
        self.advance_id();
        self.active.insert(id, repeating);
        Ok(id)
    }

    fn advance_id(&mut self) {
        self.next_id = if self.next_id == MAX_ID {
            1
        } else {
            self.next_id + 1
        };
    }

    pub fn clear(&mut self, id: f64) -> Result<bool, String> {
        if !id.is_finite() || id < 0.0 || id > u32::MAX as f64 || id.fract() != 0.0 {
            return Err(
                "clearInterval/clearTimeout currently require an unsigned integer timer ID".into(),
            );
        }
        Ok(self.active.remove(&(id as u32)).is_some())
    }

    /// Remove a one-shot registration before entering authored code. That code
    /// may clear itself, schedule another callback or restart the publication.
    pub fn fire(&mut self, id: u32) -> bool {
        let Some(repeating) = self.active.get(&id).copied() else {
            return false;
        };
        if !repeating {
            self.active.remove(&id);
        }
        true
    }

    pub fn contains(&self, id: u32) -> bool {
        self.active.contains_key(&id)
    }

    pub fn reset(&mut self) {
        self.active.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_timer_family() {
        let mut ids = ScriptTimerIds::new(2).unwrap();
        let one = ids.register(0.0, false).unwrap();
        let repeated = ids.register(12.5, true).unwrap();
        assert_ne!(one, 0);
        assert_ne!(one, repeated);
        assert!(ids.register(20.0, false).is_err());
        assert!(ids.fire(one));
        assert!(!ids.fire(one));
        assert!(ids.fire(repeated));
        assert!(ids.fire(repeated));
        assert!(ids.clear(repeated as f64).unwrap());
        assert!(!ids.fire(repeated));
        assert!(!ids.clear(0.0).unwrap());
        assert!(ids.clear(-1.0).is_err());
        assert!(ids.register(f64::NAN, false).is_err());
        let stale = ids.register(1.0, true).unwrap();
        ids.reset();
        let fresh = ids.register(1.0, true).unwrap();
        assert_ne!(fresh, stale);
        assert!(!ids.clear(stale as f64).unwrap());
        assert!(ids.contains(fresh));
        ids.next_id = MAX_ID;
        let wrapped = ids.register(1.0, false).unwrap();
        assert_eq!(wrapped, MAX_ID);
        assert!(ids.clear(wrapped as f64).unwrap());
        assert_eq!(ids.register(1.0, false).unwrap(), 1);

        // The AS2 custom-event companion uses ordered Rust listener identities.
        let mut broadcaster = crate::as_broadcaster::ScriptBroadcaster::new(2).unwrap();
        let first = broadcaster.add(0).unwrap();
        let second = broadcaster.add(0).unwrap();
        assert_eq!(broadcaster.add(first).unwrap(), first);
        assert_eq!(broadcaster.listeners(), &[first, second]);
        assert!(broadcaster.add(0).is_err());
        assert!(broadcaster.remove(first));
        assert_eq!(broadcaster.at(0), second);
        assert_eq!(broadcaster.at(1), 0);
        assert!(!broadcaster.remove(first));
        broadcaster.add(first).unwrap();
        assert_eq!(broadcaster.listeners(), &[second, first]);
        broadcaster.reset();
        assert!(broadcaster.listeners().is_empty());
    }
}
