//! TXTEXP (TXTEXPLODE): replace TEXT and MTEXT with polylines that trace the characters exactly as
//! they are drawn: the same font, height, width factor, obliquing, rotation, justification and
//! MTEXT wrapping. Outline (TrueType/OpenType) letters become closed polylines around each glyph
//! contour; stroke-font letters become the polylines of their strokes. The pieces keep the text's
//! layer, colour, linetype, lineweight and other properties, and the text is deleted. The whole
//! command is one undo step.

use draftcraft_color::{Color, Rgb};
use draftcraft_doc::{Drawing, Entity, EntityKind, Handle};
use draftcraft_geom::{PolyVertex, Vec2};
use serde_json::{Value, json};

use super::helpers::lwpoly;
use super::machines::{SelOutcome, SelectPhase};
use super::{CommandSpec, targets};
use crate::{Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("txtexp", "Explode Text", run_txtexp)
            .menu(&["Tools", "Express Tools", "Explode Text"])
            .alias(&["txtexplode"])
            .params("{handles?} (default: selection; TEXT and MTEXT, other objects are skipped) → {handles, texts}")
            .interactive(|_| Ok(Box::new(TxtExpM::default()))),
    ]
}

/// One traced piece: its points, whether it is closed, and an inline MTEXT colour if any.
pub(crate) struct Traced {
    pub points: Vec<Vec2>,
    pub closed: bool,
    pub color: Option<Color>,
}

fn mtext_color(c: Option<draftcraft_fonts::MTextColor>) -> Option<Color> {
    match c? {
        draftcraft_fonts::MTextColor::Aci(i) => u8::try_from(i).ok().filter(|i| *i > 0).map(Color::Index),
        draftcraft_fonts::MTextColor::Rgb(v) => Some(Color::True(Rgb::from_u32(v))),
    }
}

/// A polyline from a stroke or contour: repeated points dropped, a last point equal to the first
/// turned into the closing segment. `None` when nothing drawable is left.
fn piece(pts: &[Vec2], closed: bool, color: Option<Color>) -> Option<Traced> {
    let mut v: Vec<Vec2> = Vec::with_capacity(pts.len());
    for p in pts.iter().copied().filter(|p| p.is_finite()) {
        if v.last().is_none_or(|q: &Vec2| !q.near(p, 1e-12)) {
            v.push(p);
        }
    }
    let mut closed = closed;
    if v.len() >= 3
        && let (Some(a), Some(b)) = (v.first().copied(), v.last().copied())
        && a.near(b, 1e-12)
    {
        v.pop();
        closed = true;
    }
    if v.len() < 3 {
        closed = false;
    }
    (v.len() >= 2).then_some(Traced { points: v, closed, color })
}

fn trace_shaped(sh: &draftcraft_fonts::Shaped, color: Option<Color>, out: &mut Vec<Traced>) {
    out.extend(sh.strokes.iter().filter_map(|s| piece(s, false, color)));
    out.extend(sh.glyphs.iter().flatten().filter_map(|c| piece(c, true, color)));
}

/// The traced characters of a TEXT or MTEXT as it draws (an annotative one at the current
/// annotation scale), and its elevation. `None` for other objects.
pub(crate) fn trace_text(d: &Drawing, e: &Entity) -> Option<(Vec<Traced>, f64)> {
    if draftcraft_doc::is_annotative(e) {
        let mut se = match d.anno_draw(e) {
            draftcraft_doc::AnnoDraw::Scaled(f) => draftcraft_doc::anno_scaled(e, f),
            _ => e.clone(),
        };
        se.common.anno_scales.clear();
        return trace_text(d, &se);
    }
    let mut out = Vec::new();
    match &e.kind {
        EntityKind::Text(t) => {
            let (sh, _) = draftcraft_render::place_text_entity(d, t, &t.value);
            trace_shaped(&sh, None, &mut out);
            Some((out, t.insert.z))
        }
        EntityKind::MText(m) => {
            let l = draftcraft_render::layout_mtext_entity(d, m);
            for p in &l.pieces {
                trace_shaped(&p.shaped, mtext_color(p.color), &mut out);
            }
            Some((out, m.insert.z))
        }
        _ => None,
    }
}

/// Explode the text objects among `hs`. Returns the new polylines and how many texts went.
pub(crate) fn txtexp(s: &mut Session, hs: &[Handle]) -> Result<(Vec<Handle>, usize)> {
    let mut out = Vec::new();
    let mut texts = 0;
    for h in hs {
        if super::curves::is_locked(s, *h) {
            continue;
        }
        let d = s.doc()?;
        let Some(e) = d.entity(*h).map(|e| (**e).clone()) else { continue };
        let Some(space) = d.space_of(*h) else { continue };
        let Some((pieces, z)) = trace_text(d, &e) else { continue };
        let mut common = e.common.clone();
        common.xdata.clear();
        common.anno_scales.clear();
        let d = s.doc_mut()?;
        d.remove_entity(*h);
        texts += 1;
        for p in pieces {
            let mut kind = lwpoly(p.points.into_iter().map(PolyVertex::new).collect(), p.closed);
            if let EntityKind::LwPolyline(pl) = &mut kind {
                pl.elevation = z;
            }
            let mut c = common.clone();
            if let Some(col) = p.color {
                c.color = col;
            }
            let nh = d.new_handle();
            if let Some(st) = d.space_mut(&space) {
                st.push(Entity { handle: nh, common: c, kind });
                out.push(nh);
            }
        }
    }
    s.set_selection(Vec::new());
    Ok((out, texts))
}

fn run_txtexp(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let (r, n) = txtexp(s, &hs)?;
    Ok(json!({ "handles": r.iter().map(|h| h.hex()).collect::<Vec<_>>(), "texts": n }))
}

#[derive(Default)]
struct TxtExpM {
    sel: SelectPhase,
}

impl TxtExpM {
    fn finish(s: &mut Session, hs: &[Handle]) -> Result<Step> {
        let (pl, n) = txtexp(s, hs)?;
        let skipped = hs.len().saturating_sub(n);
        let mut msg =
            format!("{n} text object{} exploded into {} polyline{}", if n == 1 { "" } else { "s" }, pl.len(), if pl.len() == 1 { "" } else { "s" });
        if skipped > 0 {
            msg.push_str(&format!("; {skipped} object{} not text or on a locked layer", if skipped == 1 { " was" } else { "s were" }));
        }
        s.echo(msg);
        Ok(Step::Done)
    }
}

impl Interactive for TxtExpM {
    fn name(&self) -> &'static str {
        "TXTEXP"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            let hs = self.sel.picked.clone();
            return Self::finish(s, &hs);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        self.sel.prompt()
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.sel.feed(s, &i)? {
            SelOutcome::More => Ok(Step::Continue),
            SelOutcome::Empty => Ok(Step::Done),
            SelOutcome::Done(hs) => Self::finish(s, &hs),
        }
    }
}
