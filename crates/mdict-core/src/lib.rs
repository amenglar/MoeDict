//! Core engine for MoeDict.
//!
//! The crate is deliberately UI-agnostic: it parses MDX/MDD files, builds a
//! compact read-only index and answers queries.  No SQL engine, no async
//! runtime, no web framework.

pub mod audio;
pub mod error;
pub mod index;
pub mod mdd;
pub mod mdict;
pub mod registry;
pub mod util;

pub use error::{Error, Result};
pub use index::{IndexBuilder, IndexFile, IndexHeader, IndexProgress, IndexStats};
pub use mdd::MddReader;
pub use mdict::{KeyEntry, Mdict, read_title};
pub use registry::{
    AddOutcome, DictStatus, DictionaryInfo, DictionaryStore, QueryHit, Settings, SkippedPath,
    Suggestion,
};
