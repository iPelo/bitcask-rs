use std::collections::HashMap;

/// File and byte range for the latest record for a key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordPos {
    pub file_id: u64,
    pub offset: u64,
    pub size: u64,
    pub timestamp: u64,
}

/// In-memory index from key bytes to their latest on-disk position.
#[derive(Debug, Default)]
pub struct KeyDir {
    entries: HashMap<Vec<u8>, RecordPos>,
}

impl KeyDir {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, key: Vec<u8>, position: RecordPos) -> Option<RecordPos> {
        self.entries.insert(key, position)
    }

    pub fn get(&self, key: &[u8]) -> Option<&RecordPos> {
        self.entries.get(key)
    }

    pub fn remove(&mut self, key: &[u8]) -> Option<RecordPos> {
        self.entries.remove(key)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

