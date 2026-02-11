//! CD-path bypass and path mapping
//!
//! Redirects file paths that reference CD-ROM drives to the local game directory.
//! This is the core of the CD-check bypass.
//!
//! Configuration via environment variables:
//! - `OPENWILLY_GAME_DIR` – local game installation directory
//! - `OPENWILLY_CD_DRIVE` – original CD drive letter (default: auto-detect)

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::debug_log;

/// Local game directory (set from environment or defaults)
static GAME_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Initialize path mapping from environment variables
pub fn init() {
    let game_dir = std::env::var("OPENWILLY_GAME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            // Default: use the directory where the DLL is loaded from
            // Walk up from Xtras/ to game root
            if let Ok(exe_path) = std::env::current_exe() {
                if let Some(parent) = exe_path.parent() {
                    return parent.to_path_buf();
                }
            }
            PathBuf::from(".")
        });

    debug_log(&format!("Game directory: {}", game_dir.display()));
    let _ = GAME_DIR.set(game_dir);
}

/// Get the configured game directory
pub fn game_dir() -> &'static Path {
    GAME_DIR.get().map(|p| p.as_path()).unwrap_or(Path::new("."))
}

/// Redirect a path if it references a CD-ROM drive
///
/// Common CD drive letters (D:, E:, F:, G:) are redirected to the local game dir.
/// Relative paths and paths on the system drive are left unchanged.
pub fn redirect_path(path: &str) -> String {
    let path_trimmed = path.trim();

    if path_trimmed.is_empty() {
        return path_trimmed.to_string();
    }

    // Check for absolute paths with drive letters
    let bytes = path_trimmed.as_bytes();
    if bytes.len() >= 3 && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/') {
        let drive_letter = bytes[0].to_ascii_uppercase();

        // CD-ROM drives are typically D: and above
        // System drive is usually C:
        if drive_letter >= b'D' && drive_letter <= b'Z' {
            // Check if this drive actually exists locally
            let drive_root = format!("{}:\\", drive_letter as char);
            if !Path::new(&drive_root).exists() || is_cd_drive(&drive_root) {
                // Redirect to game directory
                let relative = &path_trimmed[3..]; // skip "X:\"
                let redirected = game_dir().join(relative);
                let result = redirected.to_string_lossy().to_string();
                debug_log(&format!("Path redirect: {} -> {}", path_trimmed, result));
                return result;
            }
        }
    }

    path_trimmed.to_string()
}

/// Check if a drive is a CD-ROM drive
fn is_cd_drive(root: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::ffi::CString;
        if let Ok(c_root) = CString::new(root) {
            unsafe {
                use windows::core::PCSTR;
                use windows::Win32::Storage::FileSystem::GetDriveTypeA;
                let drive_type = GetDriveTypeA(PCSTR::from_raw(c_root.as_ptr() as *const u8));
                // DRIVE_CDROM = 5
                return drive_type == 5;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redirect_relative_path() {
        // Relative paths should not be redirected
        let result = redirect_path("Data\\test.txt");
        assert_eq!(result, "Data\\test.txt");
    }

    #[test]
    fn test_redirect_empty() {
        assert_eq!(redirect_path(""), "");
    }

    #[test]
    fn test_c_drive_not_redirected() {
        // C: drive should not be redirected (system drive)
        let result = redirect_path("C:\\Windows\\test.txt");
        assert_eq!(result, "C:\\Windows\\test.txt");
    }
}
