//! Platform-specific desktop support.
#[cfg(target_os = "linux")]
#[path = "broker_hotkey_linux.rs"]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(target_os = "windows")]
#[path = "broker_hotkey_windows.rs"]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::*;
