//! Director Engine Support
//!
//! This crate provides support for Macromedia Director games, including:
//! - .DXR file parsing
//! - Xtra plugin wrappers
//! - Lingo script analysis

pub mod xtra;

/// Director engine version detection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectorVersion {
    Director5,
    Director6,
    Director7,
    Unknown,
}

/// Detect Director version from DXR header
pub fn detect_version(dxr_data: &[u8]) -> DirectorVersion {
    // DXR files start with specific magic bytes
    if dxr_data.len() < 4 {
        return DirectorVersion::Unknown;
    }

    // Check for Director file signatures
    // This is simplified - real detection would analyze RIFX/XFIR chunks
    match &dxr_data[0..4] {
        b"RIFX" | b"XFIR" => {
            // Would need to parse chunk data to determine exact version
            DirectorVersion::Director6 // Most likely for Willy Werkel games
        }
        _ => DirectorVersion::Unknown,
    }
}
