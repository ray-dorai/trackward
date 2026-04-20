//! In-process anchor sink for tests.
//!
//! Records every `put` into a shared map so a test can assert
//! "the anchor the DB claims it shipped is exactly these bytes". The
//! inner state is `Arc<Mutex<...>>` so cloning the sink — as the
//! `AppState` does — shares one recorder across every request handler
//! and the background anchor task.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::errors::Error;

#[derive(Clone, Default)]
pub struct MemorySink {
    inner: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl MemorySink {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn put(&self, key: &str, bytes: Vec<u8>) -> Result<String, Error> {
        self.inner
            .lock()
            .expect("memory sink poisoned")
            .insert(key.to_string(), bytes);
        Ok(format!("memory://{key}"))
    }

    /// Fetch what was previously uploaded under `key`, or `None` if
    /// nothing landed there. Test-only.
    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.inner
            .lock()
            .expect("memory sink poisoned")
            .get(key)
            .cloned()
    }
}
