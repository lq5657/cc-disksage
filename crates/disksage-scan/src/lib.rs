//! Volume and filesystem scanning.

pub mod volumes;
pub mod walker;

pub use disksage_core as core;
pub use volumes::enumerate_volumes;
pub use walker::{CancelToken, ScanError, ScanProgress, ScanRequest, scan_volume};

pub fn engine_name() -> &'static str {
    "walker"
}
