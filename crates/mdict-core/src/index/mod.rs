mod builder;
mod format;

use serde::Serialize;

pub use builder::{IndexBuilder, IndexStats};
pub use format::{FORMAT_VERSION, IndexFile, IndexHeader, read_index_header};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexProgress {
    pub phase: &'static str,
    pub current: u64,
    pub total: u64,
    pub percent: f32,
}

impl IndexProgress {
    pub fn new(phase: &'static str, current: u64, total: u64) -> Self {
        let percent = if total == 0 {
            0.0
        } else {
            (current as f64 / total as f64 * 100.0).min(100.0) as f32
        };
        Self {
            phase,
            current,
            total,
            percent,
        }
    }
}
