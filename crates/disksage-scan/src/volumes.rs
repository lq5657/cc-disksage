use disksage_core::VolumeInfo;
use sysinfo::Disks;

/// Enumerate local fixed volumes as drive-letter entries.
pub fn enumerate_volumes() -> Vec<VolumeInfo> {
    let disks = Disks::new_with_refreshed_list();
    let mut volumes = Vec::new();

    for disk in disks.list() {
        if disk.is_removable() {
            continue;
        }
        let mount = disk.mount_point().to_string_lossy();
        let Some(letter) = mount.chars().next() else {
            continue;
        };
        if !letter.is_ascii_alphabetic() {
            continue;
        }
        let total = disk.total_space();
        if total == 0 {
            continue;
        }
        volumes.push(VolumeInfo {
            letter: letter.to_ascii_uppercase(),
            label: disk.name().to_string_lossy().into_owned(),
            filesystem: disk.file_system().to_string_lossy().into_owned(),
            total_bytes: total,
            free_bytes: disk.available_space(),
        });
    }

    volumes.sort_by_key(|volume| volume.letter);
    volumes.dedup_by_key(|volume| volume.letter);
    volumes
}
