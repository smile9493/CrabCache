use crate::infra::types::HostDisk;
use std::path::Path;

/// Query host disk usage for a given mount point using `statvfs`.
pub fn get_disk_usage(path: &str) -> Result<HostDisk, String> {
    let p = Path::new(path);
    let cpath = std::ffi::CString::new(p.to_str().ok_or("non-utf8 path")?)
        .map_err(|e| format!("CString error: {}", e))?;

    let mut buf = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: cpath is a valid null-terminated string, buf is a properly aligned struct.
    let ret = unsafe { libc::statvfs(cpath.as_ptr(), buf.as_mut_ptr()) };
    if ret != 0 {
        return Err(format!(
            "statvfs({}) failed: {}",
            path,
            std::io::Error::last_os_error()
        ));
    }

    // SAFETY: statvfs succeeded, buf is initialized.
    let sb = unsafe { buf.assume_init() };
    let frsize = sb.f_frsize.max(1);
    let total = sb.f_blocks * frsize;
    let available = sb.f_bavail * frsize;
    let used = total.saturating_sub(sb.f_bfree * frsize);
    let usage_percent = if total > 0 {
        (used as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    Ok(HostDisk {
        mount_point: path.to_string(),
        total_bytes: total,
        used_bytes: used,
        available_bytes: available,
        usage_percent,
    })
}

/// Collect disk usage for default mount points.
/// Checks `/` and optionally `/var/lib/docker` (container data).
pub fn collect_host_disks() -> Vec<HostDisk> {
    let mut disks = Vec::new();
    let targets = ["/"];

    for target in &targets {
        if let Ok(disk) = get_disk_usage(target) {
            disks.push(disk);
        }
    }

    // Also check /var/lib/docker if it exists as a separate mount
    let docker_mount = std::path::Path::new("/var/lib/docker");
    if docker_mount.exists() {
        if let Ok(disk) = get_disk_usage("/var/lib/docker") {
            // Only add if different from root
            if !disks.iter().any(|d| {
                d.mount_point == "/" && d.total_bytes == disk.total_bytes
            }) {
                disks.push(disk);
            }
        }
    }

    disks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_root_disk_usage() {
        let disk = get_disk_usage("/").unwrap();
        assert!(disk.total_bytes > 0);
        assert!(disk.available_bytes > 0);
        assert!(disk.usage_percent >= 0.0 && disk.usage_percent <= 100.0);
    }

    #[test]
    fn test_collect_host_disks() {
        let disks = collect_host_disks();
        assert!(!disks.is_empty());
        assert!(disks.iter().any(|d| d.mount_point == "/"));
    }
}
