use std::path::PathBuf;

/// Identifier for a data file in the append-only log.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DataFileId(pub u64);

/// Append-only log state.
#[derive(Debug)]
pub struct Log {
    directory: PathBuf,
    active_file_id: DataFileId,
}

impl Log {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
            active_file_id: DataFileId(1),
        }
    }

    pub fn directory(&self) -> &PathBuf {
        &self.directory
    }

    pub fn active_file_id(&self) -> DataFileId {
        self.active_file_id
    }
}

