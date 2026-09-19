//! Platform boundary for desktop integrations.
//!
//! The Tauri command layer deliberately depends on this module rather than on
//! desktop APIs directly.  Windows remains in the existing implementation for
//! now; Linux starts with the pieces required for a safe, launchable baseline.

#[cfg(target_os = "linux")]
pub mod linux;
