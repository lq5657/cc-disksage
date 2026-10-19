//! Volume and filesystem scanning.

pub mod discovery;
pub mod duplicates;
#[cfg(windows)]
pub mod mft;
pub mod volumes;
pub mod walker;

pub use discovery::{
    DiscoveryItem, DiscoveryProgress, DiscoveryRequest, DiscoveryResult, run_discovery,
};
pub use disksage_core as core;
pub use duplicates::{
    DuplicateFile, DuplicateGroup, DuplicateRequest, DuplicateScanProgress, DuplicateScanResult,
    find_duplicates,
};
#[cfg(windows)]
pub use mft::{MftScanRequest, scan_volume_mft};
pub use volumes::enumerate_volumes;
pub use walker::{CancelToken, ScanError, ScanProgress, ScanRequest, scan_volume};

pub fn engine_name() -> &'static str {
    "walker"
}
