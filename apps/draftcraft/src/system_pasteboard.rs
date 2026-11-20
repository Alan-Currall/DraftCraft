//! Host system clipboard integration (macOS pasteboard).
//!
//! This module installs a callback the engine can query before Paste commands to import DWG/DXF
//! payloads or plain text from the system clipboard. The implementation here is a stub that
//! defers platform-specific support to a future update.
//!
//! On macOS, prefer reading NSPasteboard types that AutoCAD LT publishes (DWG/DXF payloads and
//! text formats) and return them through `SystemPaste`.

#[cfg(target_os = "macos")]
pub fn install() {
    // Register a no-op reader for now; a platform-specific implementation can replace this
    // with one that inspects NSPasteboard and returns SystemPaste::Drawing/Text.
    draftcraft_engine::cmd::clipboard::set_system_paste(|| None);
}

#[cfg(not(target_os = "macos"))]
pub fn install() {
    // Non-macOS platforms install nothing.
}
