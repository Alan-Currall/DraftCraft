//! The clipboard every open drawing shares (COPYCLIP, CUTCLIP, COPYBASE; PASTECLIP, PASTEORIG,
//! PASTEBLOCK).
//!
//! Like AutoCAD's, it holds the copied objects together with what they refer to in their drawing:
//! layers, linetypes, text, dimension, multileader and table styles, and the blocks they reference
//! (nested ones too). Pasting adds the ones the target drawing doesn't have; a name the target
//! already has keeps the target's definition, and a generated (anonymous `*…`) block whose name is
//! taken gets a fresh one. Pasted objects get new handles.
//!
//! The clipboard can also travel as DXF text ([`system_text`], [`load_system_text`]), so a front
//! end can put it on the system clipboard and paste it into another DraftCraft window.
//!
//! Additionally, hosts may install a reader for the system clipboard (DWG/DXF payloads or plain
//! text) via [`set_system_paste`]. When present, paste commands call [`prepare_system_paste`] to
//! load a foreign offer into the session clipboard before pasting.

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, OnceLock};

use draftcraft_doc::{Block, Drawing, Entity, EntityKind, HVal, Handle, MText};
use draftcraft_geom::{Mat3, Vec2, Vec3};

use super::{bad, has_doc};
use crate::{Result, Session};

/// More objects than this aren't mirrored to (or read from) the system clipboard.
const MAX_SYSTEM_OBJECTS: usize = 50_000;
/// Bounds the walk through nested block references.
const MAX_WALK: usize = 5_000_000;

/// Where the clipboard's objects came from, and the definitions they need.
#[derive(Clone, Debug)]
pub struct ClipSource {
    /// `DocState::uid` of the drawing they were copied from; `None` when they came from the system
    /// clipboard.
    pub uid: Option<u64>,
    /// The layers, linetypes, styles and blocks the objects reference (and the source's header).
    pub defs: Arc<Drawing>,
    /// Counts copies, so a front end mirrors each new clipboard to the system clipboard once.
    pub serial: u64,
}

/// Put copies of `ents` from the active drawing on the clipboard, with `base` as base point.
pub(crate) fn copy(s: &mut Session, ents: Vec<Entity>, base: Vec2) -> Result<()> {
    let uid = s.state()?.uid;
    let defs = defs_for(s.doc()?, &ents);
    set(s, ents, base, defs, Some(uid));
    Ok(())
}

fn set(s: &mut Session, ents: Vec<Entity>, base: Vec2, defs: Drawing, uid: Option<u64>) {
    let serial = s.clipboard_source.as_ref().map_or(0, |c| c.serial).wrapping_add(1);
    s.clipboard = ents;
    s.clipboard_base = base;
    s.clipboard_source = Some(ClipSource { uid, defs: Arc::new(defs), serial });
}

/// Enabled check of PASTECLIP and PASTEBLOCK: something to paste (session clipboard not empty)
/// or a foreign offer available on the system clipboard through the host hook.
pub(crate) fn has_clip(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.clipboard.is_empty() && !system_paste_available() { Err("the clipboard is empty".into()) } else { Ok(()) }
}

/// Enabled check of PASTEORIG: as in AutoCAD, it pastes objects copied from another drawing.
pub(crate) fn can_paste_orig(s: &Session) -> std::result::Result<(), String> {
    has_clip(s)?;
    if from_active(s) {
        return Err("the clipboard holds objects from this drawing; Paste to Original Coordinates pastes into another drawing".into());
    }
    Ok(())
}

/// Whether the clipboard was copied from the active drawing.
pub fn from_active(s: &Session) -> bool {
    let active = s.state().ok().map(|st| st.uid);
    s.clipboard_source.as_ref().is_some_and(|c| c.uid.is_some() && c.uid == active)
}

/// Paste the clipboard into the active drawing's current space, moved by `m`; selects and
/// returns the new objects.
pub(crate) fn paste(s: &mut Session, m: &Mat3) -> Result<Vec<Handle>> {
    let ents = s.clipboard.clone();
    let defs = s.clipboard_source.as_ref().map(|c| c.defs.clone());
    let space = s.space();
    let d = s.doc_mut()?;
    let renames = defs.map(|x| merge(d, &x)).unwrap_or_default();
    let mut out = Vec::new();
    for e in &ents {
        let mut e = adopt(d, e, &renames);
        e.kind.transform(m);
        d.ensure_layer(&e.common.layer);
        add_scales(d, &e);
        out.push(e.handle);
        if let Some(st) = d.space_mut(&space) {
            st.push(e);
        }
    }
    s.set_selection(out.clone());
    Ok(out)
}

/// PASTEBLOCK: the clipboard becomes a new block (named `A$C…`, base point = the clipboard's), and
/// a reference to it is inserted at `at`. Returns the block reference.
pub(crate) fn paste_block(s: &mut Session, at: Vec2) -> Result<Handle> {
    let ents = s.clipboard.clone();
    if ents.is_empty() {
        return Err(bad("pasteblock", "the clipboard is empty"));
    }
    let defs = s.clipboard_source.as_ref().map(|c| c.defs.clone());
    let base = s.clipboard_base;
    let d = s.doc_mut()?;
    let renames = defs.map(|x| merge(d, &x)).unwrap_or_default();
    let seed = d.handseed;
    let name = (0..1_000_000u64)
        .map(|i| format!("A$C{:08X}", seed.wrapping_add(i) & 0xFFFF_FFFF))
        .find(|n| d.block(n).is_none())
        .ok_or_else(|| bad("pasteblock", "no free block name"))?;
    let mut b = Block::new(&name);
    b.base = base.to3(0.0);
    for e in &ents {
        let e = adopt(d, e, &renames);
        d.ensure_layer(&e.common.layer);
        add_scales(d, &e);
        b.entities.push(e);
    }
    d.blocks.insert(name.clone(), Arc::new(b));
    let h = super::blocks::insert(s, &name, at, 1.0, 0.0, &serde_json::Map::new())?;
    s.set_selection(vec![h]);
    Ok(h)
}

/// The clipboard as DXF text (base point in `$INSBASE`), for the system clipboard. `None` when
/// it is empty, very large, or this build has no DXF writer.
pub fn system_text(s: &Session) -> Option<String> {
    if s.clipboard.is_empty() || s.clipboard.len() > MAX_SYSTEM_OBJECTS {
        return None;
    }
    let hooks = super::file::io()?;
    let d = system_drawing_impl(s);
    let bytes = (hooks.write)(&d, "clipboard.dxf").ok()?;
    String::from_utf8(bytes).ok()
}

/// A minimal Drawing holding the clipboard entities and required definitions, with $INSBASE set.
fn system_drawing_impl(s: &Session) -> Drawing {
    let mut d = Drawing::new_imperial();
    let renames = match &s.clipboard_source {
        Some(c) => {
            d.header = c.defs.header.clone();
            merge(&mut d, &c.defs)
        }
        None => BTreeMap::new(),
    };
    d.header.set("INSBASE", HVal::Point(s.clipboard_base.to3(0.0)));
    for e in &s.clipboard {
        let e = adopt(&mut d, e, &renames);
        d.ensure_layer(&e.common.layer);
        d.model.push(e);
    }
    d
}

/// The clipboard Drawing built the same way as [`system_text`], for host payload writers.
pub fn system_drawing(s: &Session) -> Option<Drawing> {
    if s.clipboard.is_empty() || s.clipboard.len() > MAX_SYSTEM_OBJECTS {
        return None;
    }
    Some(system_drawing_impl(s))
}

/// Make DXF text (from the system clipboard) the clipboard: its model space objects with what
/// they reference, the base point from `$INSBASE`. `false` (clipboard unchanged) when the text
/// isn't a DXF drawing with objects.
pub fn load_system_text(s: &mut Session, text: &str) -> bool {
    let mut lines = text.lines().map(str::trim);
    if lines.next() != Some("0") || lines.next() != Some("SECTION") || text.len() > 256 << 20 {
        return false;
    }
    let Some(hooks) = super::file::io() else { return false };
    let Ok(src) = (hooks.read)(text.as_bytes(), "clipboard.dxf") else { return false };
    take_from_drawing(s, src)
}

/// A foreign offer on the system clipboard that the desktop host found and reports.
#[derive(Clone, Debug)]
pub enum SystemPaste {
    /// A DWG or DXF payload; `name` is `clipboard.dwg` or `clipboard.dxf`.
    Drawing { bytes: Vec<u8>, name: String },
    /// Plain text, or RTF already reduced to visible text.
    Text(String),
}

static SYSTEM_PASTE: OnceLock<fn() -> Option<SystemPaste>> = OnceLock::new();

/// Install the host's system-clipboard reader (e.g. macOS pasteboard). A second call is ignored.
pub fn set_system_paste(f: fn() -> Option<SystemPaste>) {
    let _ = SYSTEM_PASTE.set(f);
}

/// Whether the host currently sees a drawing or text on the system clipboard.
pub fn system_paste_available() -> bool {
    SYSTEM_PASTE.get().and_then(|f| f()).is_some()
}

fn same_text(a: &str, b: &str) -> bool {
    a.replace("\r\n", "\n") == b.replace("\r\n", "\n")
}

/// Load a foreign drawing or text from the system clipboard into the session clipboard, when it
/// is not the copy we last put there ourselves. Returns true when something was loaded.
pub fn prepare_system_paste(s: &mut Session) -> bool {
    let Some(f) = SYSTEM_PASTE.get() else { return false };
    let ours = s.clipboard_source.as_ref().is_some_and(|c| c.uid.is_some()) && !s.clipboard.is_empty();
    // Prefer the in-app clipboard when it already holds a copy from this drawing.
    if ours {
        return false;
    }
    let Some(offer) = f() else { return false };
    let our_text = if ours { system_text(s) } else { None };
    match offer {
        SystemPaste::Drawing { bytes, name } => load_system_bytes(s, &bytes, &name),
        SystemPaste::Text(text) => {
            if let Some(t) = &our_text
                && same_text(t, &text)
            {
                return false;
            }
            if load_system_text(s, &text) { true } else { load_plain_mtext(s, &text) }
        }
    }
}

/// Make a DWG or DXF payload (from the system clipboard) the session clipboard: model-space
/// objects, or the first paper space with any, with the base point from `$INSBASE`.
pub fn load_system_bytes(s: &mut Session, bytes: &[u8], name: &str) -> bool {
    if bytes.is_empty() || bytes.len() > 256 << 20 {
        return false;
    }
    let Some(hooks) = super::file::io() else { return false };
    let Ok(src) = (hooks.read)(bytes, name) else { return false };
    take_from_drawing(s, src)
}

fn paste_entities(src: &Drawing) -> Vec<Entity> {
    if !src.model.is_empty() {
        return src.model.iter().map(|e| (**e).clone()).collect();
    }
    for layout in &src.layouts {
        if !layout.entities.is_empty() {
            return layout.entities.iter().map(|e| (**e).clone()).collect();
        }
    }
    Vec::new()
}

fn take_from_drawing(s: &mut Session, src: Drawing) -> bool {
    let ents = paste_entities(&src);
    if ents.is_empty() || ents.len() > MAX_SYSTEM_OBJECTS {
        return false;
    }
    let base = src.header.point("INSBASE").map(|p| p.xy()).filter(|p| p.is_finite()).unwrap_or(Vec2::ZERO);
    let defs = defs_for(&src, &ents);
    set(s, ents, base, defs, None);
    true
}

/// Put `text` on the clipboard as a single MTEXT (plain text, or RTF already reduced to words).
/// Newlines become `\P`; braces and backslashes are escaped. Height 2.5 in current units.
pub fn load_plain_mtext(s: &mut Session, text: &str) -> bool {
    let contents = text.replace('\r', "");
    if contents.trim().is_empty() || contents.len() > 256 << 20 {
        return false;
    }
    let mut escaped = String::new();
    for c in contents.chars() {
        match c {
            '\\' => escaped.push_str("\\\\"),
            '{' => escaped.push_str("\\{"),
            '}' => escaped.push_str("\\}"),
            '\n' => escaped.push_str("\\P"),
            c => escaped.push(c),
        }
    }
    let mut d = Drawing::new_imperial();
    let m = MText {
        insert: Vec3::ZERO,
        height: 2.5,
        width: 0.0,
        attach: 1,
        rotation: 0.0,
        style: "Standard".into(),
        contents: escaped,
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
    };
    let ents = vec![Entity::new(d.new_handle(), EntityKind::MText(m))];
    let defs = defs_for(&d, &ents);
    set(s, ents, Vec2::ZERO, defs, None);
    true
}

/// Upper-case names of the table records and blocks some objects use.
#[derive(Default)]
struct Used {
    layers: HashSet<String>,
    linetypes: HashSet<String>,
    text_styles: HashSet<String>,
    dim_styles: HashSet<String>,
    mleader_styles: HashSet<String>,
    table_styles: HashSet<String>,
    blocks: HashSet<String>,
}

fn up(n: &str) -> String {
    n.to_ascii_uppercase()
}

impl Used {
    /// Note what `e` uses; returns the blocks it references.
    fn note(&mut self, e: &Entity) -> Vec<String> {
        self.layers.insert(up(&e.common.layer));
        self.linetypes.insert(up(&e.common.linetype));
        let mut blocks = Vec::new();
        match &e.kind {
            EntityKind::Text(t) => {
                self.text_styles.insert(up(&t.style));
            }
            EntityKind::MText(t) => {
                self.text_styles.insert(up(&t.style));
            }
            EntityKind::AttDef(a) => {
                self.text_styles.insert(up(&a.text.style));
            }
            EntityKind::Insert(i) => {
                self.text_styles.extend(i.attribs.iter().map(|a| up(&a.text.style)));
                blocks.push(i.block.clone());
            }
            EntityKind::Dimension(dm) => {
                self.dim_styles.insert(up(&dm.style));
                blocks.extend(dm.block.clone());
            }
            EntityKind::Leader(l) => {
                self.dim_styles.insert(up(&l.style));
            }
            EntityKind::Tolerance(t) => {
                self.dim_styles.insert(up(&t.style));
            }
            EntityKind::MLeader(m) => {
                self.mleader_styles.insert(up(&m.style));
                self.text_styles.extend(m.text.iter().map(|t| up(&t.style)));
                blocks.extend(m.block.as_ref().map(|b| b.block.clone()));
            }
            EntityKind::Table(t) => {
                self.table_styles.insert(up(&t.style));
            }
            _ => {}
        }
        blocks
    }
}

/// The definitions `ents` need from `src`: the layers, linetypes, styles and blocks they (and the
/// blocks they reference, nested ones too) use, and `src`'s header.
fn defs_for(src: &Drawing, ents: &[Entity]) -> Drawing {
    let mut used = Used::default();
    let mut frontier: Vec<String> = ents.iter().flat_map(|e| used.note(e)).collect();
    let mut walked = 0usize;
    while let Some(name) = frontier.pop() {
        if !used.blocks.insert(up(&name)) {
            continue;
        }
        let Some(b) = src.block(&name) else { continue };
        for e in b.entities.iter() {
            walked += 1;
            if walked > MAX_WALK {
                break;
            }
            frontier.extend(used.note(e));
        }
    }
    // Records referenced by other records.
    for l in src.layers.iter().filter(|l| used.layers.contains(&up(&l.name))) {
        used.linetypes.insert(up(&l.linetype));
    }
    for t in src.linetypes.iter().filter(|t| used.linetypes.contains(&up(&t.name))) {
        used.text_styles.extend(t.pattern.iter().filter_map(|p| p.style.as_deref()).map(up));
    }
    for ds in src.dim_styles.iter().filter(|t| used.dim_styles.contains(&up(&t.name))) {
        used.text_styles.insert(up(&ds.text_style));
    }
    for ms in src.mleader_styles.iter().filter(|t| used.mleader_styles.contains(&up(&t.name))) {
        used.text_styles.insert(up(&ms.text_style));
    }
    fn pick<T: Clone>(all: &[T], used: &HashSet<String>, name: fn(&T) -> &str) -> Vec<T> {
        all.iter().filter(|t| used.contains(&up(name(t)))).cloned().collect()
    }
    let mut d = Drawing::new_imperial();
    d.header = src.header.clone();
    d.layers = pick(&src.layers, &used.layers, |l| &l.name);
    d.linetypes = pick(&src.linetypes, &used.linetypes, |l| &l.name);
    d.text_styles = pick(&src.text_styles, &used.text_styles, |t| &t.name);
    d.dim_styles = pick(&src.dim_styles, &used.dim_styles, |t| &t.name);
    d.mleader_styles = pick(&src.mleader_styles, &used.mleader_styles, |t| &t.name);
    d.table_styles = pick(&src.table_styles, &used.table_styles, |t| &t.name);
    d.blocks = src.blocks.iter().filter(|(k, _)| used.blocks.contains(&up(k))).map(|(k, b)| (k.clone(), b.clone())).collect();
    d.layouts.clear();
    d
}

/// Add `defs`' table records and blocks that `d` doesn't have. Returns the blocks copied, from
/// their name in `defs` to their name in `d`.
fn merge(d: &mut Drawing, defs: &Drawing) -> BTreeMap<String, String> {
    use crate::cmd::merge::merge_named as add;
    add(&mut d.layers, &defs.layers, |l| &l.name);
    add(&mut d.linetypes, &defs.linetypes, |l| &l.name);
    add(&mut d.text_styles, &defs.text_styles, |t| &t.name);
    add(&mut d.dim_styles, &defs.dim_styles, |t| &t.name);
    add(&mut d.mleader_styles, &defs.mleader_styles, |t| &t.name);
    add(&mut d.table_styles, &defs.table_styles, |t| &t.name);
    let renames = crate::cmd::merge::block_renames(d, defs, None);
    for (from, to) in &renames {
        let Some(b) = defs.blocks.get(from) else { continue };
        let mut nb = (**b).clone();
        nb.name = to.clone();
        nb.entities = Default::default();
        for e in b.entities.iter() {
            let e = adopt(d, e, &renames);
            d.ensure_layer(&e.common.layer);
            nb.entities.push(e);
        }
        d.blocks.insert(to.clone(), Arc::new(nb));
    }
    renames
}

/// As AutoCAD does when pasting annotative objects: the annotation scales `e` supports join the
/// drawing's scale list when it has no scale of that name.
fn add_scales(d: &mut Drawing, e: &Entity) {
    for sc in &e.common.anno_scales {
        if sc.is_valid() && d.anno_scale(&sc.name).is_none() && d.scales.len() < draftcraft_doc::MAX_SCALES {
            d.scales.push(sc.clone());
        }
    }
}

/// A copy of a clipboard object for `d`: a fresh handle, block references renamed, and links to
/// objects of the source drawing (associative dimension points) dropped.
fn adopt(d: &mut Drawing, e: &Entity, renames: &BTreeMap<String, String>) -> Entity {
    let mut e = e.clone();
    e.handle = d.new_handle();
    match &mut e.kind {
        EntityKind::Insert(i) => {
            if let Some(n) = renames.get(&i.block) {
                i.block = n.clone();
            }
        }
        EntityKind::Dimension(dm) => {
            dm.assoc.clear();
            if let Some(n) = dm.block.as_ref().and_then(|b| renames.get(b)) {
                dm.block = Some(n.clone());
            }
        }
        _ => {}
    }
    e
}
