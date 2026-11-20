//! User plot style tables: files in a user folder offered to every drawing without being saved
//! with it. Supports `.ctb`/`.stb` (best-effort; v1 parses JSON-bodied payloads) and our own
//! JSON format (`PlotStyleTable::to_file_text`).
//!
//! - The folder is configurable via OPTIONS (`PLOTSTYLEPATH`); the default is the DraftCraft
//!   settings directory's `plotstyles` subfolder (`settings_dir::current()/plotstyles`).
//! - STYLESMANAGER lists these tables beside built-ins and tables kept with the drawing.
//! - Picking one for a layout only stores its name in the page setup; the file is not embedded.
//!
//! The module also registers a provider with `draftcraft_doc` so plotting and renders can
//! resolve a user table by name.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::{Result, Session, atomic_write};
use crate::cmd::{CommandSpec, bad};

static PATH_OVERRIDE: Mutex<String> = Mutex::new(String::new());
static PROVIDER_INSTALLED: OnceLock<()> = OnceLock::new();
static DRAWING_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Register the global provider once so `draftcraft_doc::plot_style_table` can resolve user tables.
pub fn register_provider_once() {
    if PROVIDER_INSTALLED.set(()).is_ok() {
        let provider = draftcraft_doc::PlotStyleProvider {
            list_names: list_names,
            get_by_name: get_by_name,
        };
        draftcraft_doc::set_plot_style_provider(provider);
    }
}

/// Hint the current drawing's folder for provider lookups.
pub fn set_current_drawing_dir(dir: Option<&Path>) {
    if let Ok(mut g) = DRAWING_DIR.lock() {
        *g = dir.map(|p| p.to_path_buf());
    }
}

/// The user plot styles folder: `PLOTSTYLEPATH` when set (OPTIONS), else the settings folder's
/// `plotstyles` subfolder.
pub fn user_folder() -> Option<PathBuf> {
    let set = PATH_OVERRIDE.lock().ok().and_then(|s| {
        let p = s.trim();
        (!p.is_empty()).then(|| PathBuf::from(p))
    });
    if set.is_some() {
        return set;
    }
    crate::settings_dir::current().map(|d| d.join("plotstyles"))
}

/// Change the user plot styles folder (OPTIONS `PLOTSTYLEPATH`).
pub fn set_user_folder(path: &str) {
    if let Ok(mut p) = PATH_OVERRIDE.lock() {
        *p = path.trim().to_string();
    }
}

fn want_file(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()) else { return false };
    matches!(ext.as_str(), "ctb" | "stb" | "json")
}

/// List user tables by file name (case preserved).
pub fn list() -> Vec<String> {
    let mut out = Vec::new();
    let dirs: Vec<PathBuf> = [DRAWING_DIR.lock().ok().and_then(|o| o.clone()), user_folder()].into_iter().flatten().collect();
    for dir in dirs {
        if let Ok(read) = std::fs::read_dir(dir) {
            for e in read.flatten().take(10_000) {
                let p = e.path();
                if e.file_type().is_ok_and(|t| t.is_file()) && want_file(&p) {
                    if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                        if !out.iter().any(|n: &String| n.eq_ignore_ascii_case(name)) {
                            out.push(name.to_string());
                        }
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()));
    out
}

/// Resolve a user table by name (case-insensitive).
pub fn resolve_path(name: &str) -> Option<PathBuf> {
    let lower = name.trim().to_ascii_lowercase();
    // Drawing folder wins, then user folder.
    if let Some(dir) = DRAWING_DIR.lock().ok().and_then(|o| o.clone()) {
        if let Ok(read) = std::fs::read_dir(dir) {
            for e in read.flatten().take(10_000) {
                let p = e.path();
                let fname = p.file_name().and_then(|n| n.to_str()).map(|s| s.to_ascii_lowercase());
                if e.file_type().is_ok_and(|t| t.is_file()) && want_file(&p) && fname.as_deref() == Some(lower.as_str()) {
                    return Some(p);
                }
            }
        }
    }
    if let Some(dir) = user_folder() {
        if let Ok(read) = std::fs::read_dir(dir) {
            for e in read.flatten().take(10_000) {
                let p = e.path();
                let fname = p.file_name().and_then(|n| n.to_str()).map(|s| s.to_ascii_lowercase());
                if e.file_type().is_ok_and(|t| t.is_file()) && want_file(&p) && fname.as_deref() == Some(lower.as_str()) {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// Provider hook: list names.
fn list_names() -> Vec<String> {
    list()
}
/// Provider hook: get one by name.
fn get_by_name(name: &str) -> Option<draftcraft_doc::PlotStyleTable> {
    let Some(path) = resolve_path(name) else { return None };
    let Ok(bytes) = std::fs::read(&path) else { return None };
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()) {
        Some(ext) if ext == "json" => {
            let Ok(text) = String::from_utf8(bytes) else { return None };
            draftcraft_doc::PlotStyleTable::from_file_text(&text).ok()
        }
        Some(ext) if ext == "ctb" || ext == "stb" => draftcraft_doc::parse_ctb_or_stb(
            path.file_name().and_then(|n| n.to_str()).unwrap_or(name),
            &bytes,
        )
        .ok(),
        _ => None,
    }
}

/// Copy `path` into the user folder (preserving the file name).
fn import_file(path: &str) -> Result<String> {
    let src = Path::new(path);
    let name = src.file_name().and_then(|n| n.to_str()).ok_or_else(|| bad("stylesmanager", "invalid file path"))?;
    let Some(dir) = user_folder() else { return Err(bad("stylesmanager", "no plot styles folder")); };
    let dst = dir.join(name);
    let bytes = std::fs::read(src).map_err(|e| bad("stylesmanager", format!("{path}: {e}")))?;
    atomic_write::write_atomic_making_folders(&dst, &bytes).map_err(|e| bad("stylesmanager", format!("{:?}: {e}", dst.display())))?;
    Ok(name.to_string())
}

// ----- stylesmanager extensions -----

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("stylesmanager.import", "Import Plot Style", run_import)
            .params("{path: file to copy into the user plot styles folder}"),
        CommandSpec::new("stylesmanager.reload", "Reload Plot Styles", run_reload).params("{}"),
    ]
}

fn run_import(_s: &mut Session, p: &Value) -> Result<Value> {
    let Some(path) = p.get("path").and_then(Value::as_str) else { return Err(bad("stylesmanager.import", "`path` is required")); };
    let name = import_file(path)?;
    Ok(json!({ "imported": name }))
}

fn run_reload(_s: &mut Session, _p: &Value) -> Result<Value> {
    // No-op for now (no caching); present to wire the UI and for parity with future caches.
    Ok(json!({ "reloaded": true }))
}

