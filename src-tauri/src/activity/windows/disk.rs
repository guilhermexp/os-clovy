//! Filesystem statistics for activity capture storage volumes on Windows.

use std::path::Path;

/// Returns free disk bytes on the volume holding `path`.
/// Stub for 1.5/RED. Real implementation in GREEN 4.2.
pub(crate) fn free_bytes(_path: &Path) -> Option<u64> {
    None
}
