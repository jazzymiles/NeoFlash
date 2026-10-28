//! AS2 AsBroadcaster listener ordering/identity, adapted from Ruffle's MIT
//! broadcaster contracts into bounded owned Rust state. Browser object identity
//! stays in the host; this state holds only opaque integer handles.
//! Source and partial boundaries: docs/RUNTIME-EASING-PROVENANCE.md.

pub struct ScriptBroadcaster {
    listeners: Vec<u32>,
    next_id: u32,
    limit: usize,
}

impl ScriptBroadcaster {
    pub fn new(limit: usize) -> Result<Self, String> {
        if !(1..=10_000).contains(&limit) {
            return Err("AsBroadcaster listener limit must be from 1 through 10000".into());
        }
        Ok(Self {
            listeners: Vec::new(),
            next_id: 1,
            limit,
        })
    }

    /// Zero requests a new identity; a previously returned identity may be
    /// added again after removal. Duplicate listeners keep their position.
    pub fn add(&mut self, existing_id: u32) -> Result<u32, String> {
        if existing_id != 0 && existing_id >= self.next_id {
            return Err(
                "AsBroadcaster listener identity does not belong to this broadcaster".into(),
            );
        }
        if self.listeners.contains(&existing_id) {
            return Ok(existing_id);
        }
        if self.listeners.len() >= self.limit {
            return Err(format!(
                "AsBroadcaster supports at most {} listeners",
                self.limit
            ));
        }
        let id = if existing_id == 0 {
            let id = self.next_id;
            self.next_id = self.next_id.checked_add(1).ok_or(
                "AsBroadcaster listener identity limit reached; reinitialize the broadcaster",
            )?;
            id
        } else {
            existing_id
        };
        self.listeners.push(id);
        Ok(id)
    }

    pub fn remove(&mut self, id: u32) -> bool {
        let Some(index) = self.listeners.iter().position(|value| *value == id) else {
            return false;
        };
        self.listeners.remove(index);
        true
    }

    pub fn listeners(&self) -> &[u32] {
        &self.listeners
    }

    /// Broadcast captures the entry length, then resolves each live index.
    /// Removing a listener during dispatch therefore follows AS2's array-order
    /// semantics instead of inventing an independent dispatch snapshot.
    pub fn at(&self, index: usize) -> u32 {
        self.listeners.get(index).copied().unwrap_or(0)
    }

    pub fn reset(&mut self) {
        self.listeners.clear();
    }
}
