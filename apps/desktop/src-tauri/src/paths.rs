//! Per-user locations: config in `%APPDATA%\Granary`, logs and runtime state in `%LOCALAPPDATA%\Granary`.
use std::path::PathBuf;

pub const APP_DIR: &str = "Granary";

fn known(var: &str) -> PathBuf {
    // Both variables always exist in a Windows user session; fall back to the temp dir rather than fail startup.
    std::env::var_os(var)
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(APP_DIR)
}

/// `%APPDATA%\Granary` (roaming): settings, server list.
pub fn config_dir() -> PathBuf {
    known("APPDATA")
}

/// `%LOCALAPPDATA%\Granary` (machine-local): logs, caches, display restore marker.
pub fn local_dir() -> PathBuf {
    known("LOCALAPPDATA")
}

/// `%LOCALAPPDATA%\Granary\logs`.
pub fn log_dir() -> PathBuf {
    local_dir().join("logs")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirs_end_with_app_name() {
        assert!(config_dir().ends_with(APP_DIR));
        assert!(log_dir().ends_with(PathBuf::from(APP_DIR).join("logs")));
    }
}
