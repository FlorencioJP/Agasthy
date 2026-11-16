//! Disk analysis and safe cleanup.
//!
//! * Candidates are only build artifacts, dependency folders and caches that
//!   can be regenerated, in user-approved project folders or a fixed list of
//!   cache locations. Source code and repositories are never candidates.
//! * Every candidate is re-validated immediately before it is touched.
//! * Removal means moving into AGASTHY's quarantine on the same volume, which
//!   can be restored. Space is reclaimed only when the quarantine entry is
//!   explicitly deleted.
//! * Docker: build cache and dangling images can be pruned on request.
//!   Volumes are never touched.

pub mod candidates;
pub mod docker;
pub mod quarantine;
pub mod safety;

pub use candidates::{
    AnalysisInput, CleanupCandidate, CleanupCategory, DiskAnalysis, SafetyLevel, analyze,
};
pub use quarantine::{Quarantine, QuarantineItem, QuarantineOp, QuarantineState};

/// Size of a directory tree without following symlinks, bounded by `max_entries`.
/// Returns `(bytes, complete)`.
pub fn dir_size(path: &std::path::Path, max_entries: usize) -> (u64, bool) {
    let mut total = 0u64;
    let mut n = 0usize;
    for e in walkdir::WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        n += 1;
        if n > max_entries {
            return (total, false);
        }
        if e.file_type().is_file()
            && let Ok(m) = e.metadata()
        {
            total += m.len();
        }
    }
    (total, true)
}
