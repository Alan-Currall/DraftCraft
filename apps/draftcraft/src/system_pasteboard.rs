//! Host system clipboard integration (macOS pasteboard).
//!
//! - Read: recognise AutoCAD LT’s public pasteboard types and DWG/DXF/text payloads, returning
//!   a SystemPaste offer to the engine on demand.
//! - Write: when the in-app clipboard updates, publish DWG bytes under the DWG pasteboard type
//!   plus DXF text under `public.utf8-plain-text`, so LT can paste DraftCraft objects as objects.
//!
//! Clean-room: type names and selection heuristics derive from inspecting the pasteboard types
//! present when LT copies, and from public platform documentation. No Autodesk code is used.

// Shared helpers for selection logic, testable headlessly.
#[cfg(any(test, target_os = "macos"))]
fn sp_looks_like_dwg(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && &bytes[0..2] == b"AC"
}
#[cfg(any(test, target_os = "macos"))]
fn sp_looks_like_dxf(bytes: &[u8]) -> bool {
    if bytes.starts_with(b"AutoCAD Binary DXF") {
        return true;
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        let s = s.trim_start_matches('\u{feff}');
        s.starts_with("0\nSECTION")
    } else {
        false
    }
}
#[cfg(any(test, target_os = "macos"))]
fn sp_is_cad_drawing_type(name: &str) -> bool {
    let n = name.trim().to_ascii_lowercase();
    n == "com.autodesk.autocad.drawing"
        || n.starts_with("autocad.r")
        || n == "cf_idrop.dwg"
        || n == "embedded object"
        || n == "embed source"
        || n == "native"
        || n == "nspasteboardtypefileurl"
}
#[cfg(any(test, target_os = "macos"))]
fn sp_is_text_type(name: &str) -> bool {
    let n = name.trim().to_ascii_lowercase();
    n == "public.utf8-plain-text" || n == "nspasteboardtypestring" || n == "nspasteboardtypertf" || n == "public.rtf"
}
#[cfg(any(test, target_os = "macos"))]
fn choose_system_paste(items: &[(String, Vec<u8>)]) -> Option<draftcraft_engine::cmd::clipboard::SystemPaste> {
    use draftcraft_engine::cmd::clipboard::SystemPaste;
    for (name, bytes) in items {
        if sp_is_cad_drawing_type(name) {
            if sp_looks_like_dwg(bytes) {
                return Some(SystemPaste::Drawing { bytes: bytes.clone(), name: "clipboard.dwg".into() });
            }
            if sp_looks_like_dxf(bytes) {
                return Some(SystemPaste::Drawing { bytes: bytes.clone(), name: "clipboard.dxf".into() });
            }
        }
    }
    for (_, bytes) in items {
        if sp_looks_like_dwg(bytes) {
            return Some(SystemPaste::Drawing { bytes: bytes.clone(), name: "clipboard.dwg".into() });
        }
    }
    for (_, bytes) in items {
        if sp_looks_like_dxf(bytes) {
            return Some(SystemPaste::Drawing { bytes: bytes.clone(), name: "clipboard.dxf".into() });
        }
    }
    for (name, bytes) in items {
        if sp_is_text_type(name) {
            if let Ok(s) = String::from_utf8(bytes.clone()) {
                return Some(SystemPaste::Text(s));
            }
        }
    }
    for (_, bytes) in items {
        if let Ok(s) = String::from_utf8(bytes.clone()) {
            return Some(SystemPaste::Text(s));
        }
    }
    None
}

#[cfg(target_os = "macos")]
mod mac {
    use std::sync::Mutex;

    use draftcraft_engine::cmd::clipboard::{SystemPaste, system_drawing, system_text};
    use objc2_app_kit::NSPasteboard;
    use objc2_foundation::{NSArray, NSData, NSString};

    struct Cache {
        change: isize,
        value: Option<SystemPaste>,
    }
    static CACHE: Mutex<Option<Cache>> = Mutex::new(None);
    static LAST_PUBLISHED: Mutex<Option<u64>> = Mutex::new(None);

    pub fn install() {
        draftcraft_engine::cmd::clipboard::set_system_paste(read_general);
    }

    fn read_general() -> Option<SystemPaste> {
        inspect(&unsafe { NSPasteboard::generalPasteboard() })
    }

    fn extract_system_paste(items: &[(String, Vec<u8>)]) -> Option<SystemPaste> {
        super::choose_system_paste(items)
    }

    fn inspect(pb: &NSPasteboard) -> Option<SystemPaste> {
        let change = pb.changeCount();
        if let Ok(guard) = CACHE.lock()
            && let Some(c) = guard.as_ref()
            && c.change == change
        {
            return c.value.clone();
        }
        let types = unsafe { pb.types()? };
        let mut items: Vec<(String, Vec<u8>)> = Vec::new();
        for i in 0..types.len() {
            let typ: *mut objc2_foundation::NSPasteboardType = unsafe { types.objectAtIndex(i) };
            let name = unsafe { NSString::from_id(typ.cast()).to_string() };
            let data: Option<*mut NSData> = unsafe { pb.dataForType(&*(typ.cast())) };
            let bytes = data.map(|d| unsafe { NSData::from_id(d).to_vec() }).unwrap_or_default();
            items.push((name, bytes));
        }
        let value = extract_system_paste(&items);
        if let Ok(mut guard) = CACHE.lock() {
            *guard = Some(Cache { change, value: value.clone() });
        }
        value
    }

    /// Publish the current DraftCraft clipboard to the macOS pasteboard (DWG + UTF‑8 text).
    pub fn mirror_if_new(session: &draftcraft_engine::Session) {
        let Some(src) = &session.clipboard_source else { return };
        let serial = src.serial;
        if let Ok(mut last) = LAST_PUBLISHED.lock() {
            if last.is_some_and(|s| s == serial) {
                return;
            }
            // Try DWG bytes first; always publish UTF‑8 text too.
            let pb = unsafe { NSPasteboard::generalPasteboard() };
            unsafe { pb.clearContents() };
            if let Some(d) = system_drawing(session) {
                if let Ok(bytes) = draftcraft_io::write(&d, "clipboard.dwg") {
                    let t = unsafe { NSString::from_str("com.autodesk.autocad.drawing") };
                    let data = unsafe { NSData::from_vec(bytes) };
                    let _: bool = unsafe { pb.setData_forType(Some(&data), Some(&t)) }.into();
                }
            }
            if let Some(text) = system_text(session) {
                let t = unsafe { NSString::from_str("public.utf8-plain-text") };
                let s = unsafe { NSString::from_str(&text) };
                let data = unsafe { NSData::from_vec(s.as_bytes().to_vec()) };
                let _: bool = unsafe { pb.setData_forType(Some(&data), Some(&t)) }.into();
            }
            *last = Some(serial);
        }
    }
}

#[cfg(target_os = "macos")]
pub use mac::install;

#[cfg(target_os = "macos")]
pub fn mirror_if_new(session: &draftcraft_engine::Session) {
    mac::mirror_if_new(session);
}

#[cfg(not(target_os = "macos"))]
pub fn install() {}

#[cfg(not(target_os = "macos"))]
pub fn mirror_if_new(_session: &draftcraft_engine::Session) {}

#[cfg(test)]
mod tests {
    use super::*;
    use draftcraft_engine::cmd::clipboard::SystemPaste;

    #[test]
    fn chooser_prefers_cad_type_and_detects_magic() {
        // DWG bytes with an AC10xx header
        let dwg = b"AC1027\x00rest".to_vec();
        let text = b"0\nSECTION\n2\nHEADER\n0\nENDSEC\n0\nEOF\n".to_vec();
        // Prefer explicit DWG type first
        let items = vec![
            ("public.utf8-plain-text".into(), text.clone()),
            ("com.autodesk.autocad.drawing".into(), dwg.clone()),
        ];
        let p = choose_system_paste(&items).expect("offer");
        match p {
            SystemPaste::Drawing { name, bytes } => {
                assert_eq!(name, "clipboard.dwg");
                assert_eq!(bytes, dwg);
            }
            _ => panic!("expected Drawing"),
        }
        // Magic fallback DWG
        let items = vec![("other/type".into(), dwg.clone())];
        assert!(matches!(choose_system_paste(&items), Some(SystemPaste::Drawing { .. })));
        // Magic fallback DXF
        let items = vec![("other/type".into(), text.clone())];
        assert!(matches!(choose_system_paste(&items), Some(SystemPaste::Drawing { name, .. }) if name.ends_with(".dxf")));
        // Text fallback when nothing else fits
        let plain = b"Hello".to_vec();
        let items = vec![("public.utf8-plain-text".into(), plain.clone())];
        assert!(matches!(choose_system_paste(&items), Some(SystemPaste::Text(t)) if t == "Hello"));
    }
}
