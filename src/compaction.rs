use crate::log::DataFileId;

/// Files selected for a compaction pass.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CompactionPlan {
    pub input_files: Vec<DataFileId>,
}

impl CompactionPlan {
    pub fn new(input_files: Vec<DataFileId>) -> Self {
        Self { input_files }
    }

    pub fn is_empty(&self) -> bool {
        self.input_files.is_empty()
    }
}
