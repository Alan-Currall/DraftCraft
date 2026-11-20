//! DraftCraft Help (F1, HELP, Help ▸ DraftCraft Help): ask a question in your own words,
//! such as "How do I draw a line?", and the Help panel shows the tool's icon, the steps,
//! where to find it, and a button that starts it. It works offline from a built-in index:
//!
//! - the hand-written topics in [`entries`] (icon, steps, the questions people ask), and
//! - every other command, from its command spec (label, menu path, aliases), so Help finds all
//!   of them even before they have a written topic.
//!
//! A question typed at the command line ("how do I trim a line", or anything ending in `?`)
//! opens Help with that question instead of failing as an unknown command.

use std::collections::HashMap;

use egui::{RichText, Sense, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::icons::{self, Icon};
use crate::theme::Tokens;

/// The hand-written help topics.
pub mod entries;

pub use entries::{ENTRIES, GROUPS};

/// One hand-written help topic for a command.
#[derive(Debug)]
pub struct Entry {
    /// The command (or UI command) id that "Start" runs.
    pub command: &'static str,
    pub title: &'static str,
    /// One of [`GROUPS`].
    pub group: &'static str,
    /// The icon its tool button shows.
    pub icon: Icon,
    pub summary: &'static str,
    pub steps: &'static [&'static str],
    /// Ways people ask for it ("draw a line", "join two points").
    pub asks: &'static [&'static str],
    pub tip: &'static str,
    /// Command ids of related topics.
    pub related: &'static [&'static str],
}

/// The Help panel's state (not saved).
#[derive(Clone, Debug, Default)]
pub struct State {
    pub query: String,
    /// The topic shown in full (a command id); `None` shows the best match.
    pub selected: Option<&'static str>,
    /// Put the keyboard focus in the question box on the next frame.
    pub focus: bool,
}

/// A help topic: a written [`Entry`], or a command known only from its spec.
#[derive(Clone, Copy, Debug)]
pub struct Topic {
    pub command: &'static str,
    pub title: &'static str,
    pub entry: Option<&'static Entry>,
}

impl Topic {
    pub fn icon(&self) -> Option<Icon> {
        self.entry.map(|e| e.icon).or_else(|| tool_icon(self.command))
    }
}

/// A search result.
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub topic: Topic,
    pub score: f32,
}

/// The written topic for `command`, if there is one.
pub fn entry(command: &str) -> Option<&'static Entry> {
    ENTRIES.iter().find(|e| e.command == command)
}

/// Words that carry no meaning in a question ("how do I ... a ...").
const STOP_WORDS: &[&str] = &[
    "a",
    "an",
    "the",
    "how",
    "do",
    "does",
    "did",
    "i",
    "you",
    "we",
    "can",
    "could",
    "would",
    "should",
    "to",
    "of",
    "in",
    "on",
    "at",
    "for",
    "with",
    "is",
    "are",
    "it",
    "its",
    "this",
    "that",
    "these",
    "those",
    "my",
    "me",
    "please",
    "want",
    "wanna",
    "like",
    "what",
    "where",
    "which",
    "why",
    "when",
    "way",
    "use",
    "using",
    "into",
    "onto",
    "from",
    "and",
    "or",
    "some",
    "there",
    "find",
    "get",
    "need",
    "go",
    "tell",
    "show",
    "help",
    "draftcraft",
];

/// First words that make a command-line entry a question for Help.
const QUESTION_WORDS: &[&str] = &["how", "what", "where", "why", "which", "when", "can", "could", "is", "does", "whats", "hows"];

/// US spellings and everyday words folded onto the words the index uses.
const FOLD: &[(&str, &str)] =
    &[("center", "centre"), ("color", "colour"), ("gray", "grey"), ("delet", "delete"), ("remov", "remove"), ("print", "plot"), ("txt", "text")];

fn stem(word: &str) -> String {
    let n = word.chars().count();
    let cut = |suffix: &str| word.strip_suffix(suffix).unwrap_or(word);
    let s = if n > 5 && word.ends_with("ing") {
        cut("ing").to_string()
    } else if n > 4 && word.ends_with("ies") {
        format!("{}y", cut("ies"))
    } else if n > 4 && ["ches", "shes", "xes", "sses"].iter().any(|e| word.ends_with(e)) {
        cut("es").to_string()
    } else if n > 3 && word.ends_with('s') && !word.ends_with("ss") {
        cut("s").to_string()
    } else if n > 4 && word.ends_with("ed") {
        cut("ed").to_string()
    } else {
        word.to_string()
    };
    FOLD.iter().find(|(from, _)| *from == s).map_or(s, |(_, to)| (*to).to_string())
}

/// Lower-case words of `text`, stemmed, without punctuation.
fn words(text: &str) -> Vec<String> {
    text.to_lowercase().split(|c: char| !(c.is_alphanumeric() || c == '-')).map(|w| w.trim_matches('-')).filter(|w| !w.is_empty()).map(stem).collect()
}

/// The meaningful words of a question: [`words`] without [`STOP_WORDS`] (all of them when
/// nothing else is left, so `I` still finds INSERT).
pub fn query_words(text: &str) -> Vec<String> {
    let all = words(text);
    let kept: Vec<String> = all.iter().filter(|w| !STOP_WORDS.contains(&w.as_str())).cloned().collect();
    if kept.is_empty() { all } else { kept }
}

/// A topic's searchable words, each with its weight (the strongest wins), and its asks.
struct Doc {
    topic: Topic,
    terms: HashMap<String, f32>,
    asks: Vec<Vec<String>>,
}

impl Doc {
    fn add(&mut self, text: &str, weight: f32) {
        for w in words(text) {
            if STOP_WORDS.contains(&w.as_str()) && weight < 6.0 {
                continue;
            }
            let e = self.terms.entry(w).or_insert(0.0);
            if *e < weight {
                *e = weight;
            }
        }
    }
}

const NAME: f32 = 6.0;
const TITLE: f32 = 3.0;
const ASK: f32 = 2.0;
const MENU: f32 = 1.5;
const GROUP: f32 = 1.0;
const SUMMARY: f32 = 0.75;

fn spec_aliases(command: &str) -> &'static [&'static str] {
    draftcraft_engine::find_command(command).map_or(&[], |s| s.aliases)
}

fn docs() -> &'static [Doc] {
    static DOCS: std::sync::OnceLock<Vec<Doc>> = std::sync::OnceLock::new();
    DOCS.get_or_init(|| {
        let mut out = Vec::new();
        for e in ENTRIES {
            let mut d = Doc { topic: Topic { command: e.command, title: e.title, entry: Some(e) }, terms: HashMap::new(), asks: Vec::new() };
            d.add(e.command, NAME);
            for a in spec_aliases(e.command) {
                d.add(a, NAME);
            }
            d.add(e.title, TITLE);
            for a in e.asks {
                d.add(a, ASK);
                d.asks.push(query_words(a));
            }
            d.add(e.group, GROUP);
            d.add(e.summary, SUMMARY);
            out.push(d);
        }
        for s in draftcraft_engine::command_specs() {
            // Sub-modes (`zoom.window`) are reached through their command.
            if s.id.contains('.') || s.label.is_empty() || entry(s.id).is_some() {
                continue;
            }
            let mut d = Doc { topic: Topic { command: s.id, title: s.label, entry: None }, terms: HashMap::new(), asks: Vec::new() };
            d.add(s.id, NAME);
            for a in s.aliases {
                d.add(a, NAME);
            }
            d.add(s.label, TITLE);
            for m in s.menu {
                d.add(m, MENU);
            }
            out.push(d);
        }
        out
    })
}

/// Edit distance between two short words, `None` when over `max`.
fn distance(a: &str, b: &str, max: usize) -> Option<usize> {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > max || a.len() > 40 || b.len() > 40 {
        return None;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            let sub = prev.get(j).copied().unwrap_or(usize::MAX / 2) + usize::from(ca != cb);
            let del = prev.get(j + 1).copied().unwrap_or(usize::MAX / 2) + 1;
            let ins = cur.get(j).copied().unwrap_or(usize::MAX / 2) + 1;
            if let Some(c) = cur.get_mut(j + 1) {
                *c = sub.min(del).min(ins);
            }
        }
        prev = cur;
    }
    prev.last().copied().filter(|d| *d <= max)
}

/// How well a query word matches an index word: 1 for the same word, less for a typed start of
/// it or a slip of a letter or two.
fn similarity(q: &str, d: &str) -> f32 {
    if q == d {
        return 1.0;
    }
    if q.len() >= 3 && d.starts_with(q) {
        return 0.7;
    }
    let n = q.chars().count().min(d.chars().count());
    if n >= 4 && distance(q, d, 1).is_some() {
        return 0.6;
    }
    if n >= 7 && distance(q, d, 2).is_some() {
        return 0.4;
    }
    0.0
}

fn score(doc: &Doc, query: &[String]) -> f32 {
    let mut total = 0.0;
    let mut matched = 0usize;
    for q in query {
        let best = doc.terms.iter().map(|(d, w)| w * similarity(q, d)).fold(0.0_f32, f32::max);
        if best > 0.0 {
            matched += 1;
        }
        total += best;
    }
    if matched == 0 {
        return 0.0;
    }
    // A question worded like one of the topic's asks.
    let mut phrase: f32 = 0.0;
    for ask in &doc.asks {
        if ask.is_empty() {
            continue;
        }
        if ask.as_slice() == query {
            phrase = phrase.max(8.0);
        } else if ask.iter().all(|w| query.contains(w)) {
            phrase = phrase.max(4.0);
        }
    }
    let coverage = matched as f32 / query.len().max(1) as f32;
    let written = if doc.topic.entry.is_some() { 0.5 } else { 0.0 };
    (total + phrase) * (0.5 + 0.5 * coverage) + written
}

/// The weakest score shown as a match.
const MIN_SCORE: f32 = 1.5;

/// The topics that best answer `question`, best first (at most `limit`).
pub fn search(question: &str, limit: usize) -> Vec<Hit> {
    let query = query_words(question);
    if query.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<Hit> = docs().iter().map(|d| Hit { topic: d.topic, score: score(d, &query) }).filter(|h| h.score >= MIN_SCORE).collect();
    hits.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.topic.command.cmp(b.topic.command)));
    hits.truncate(limit);
    hits
}

/// The topic for a command id (written or from its spec).
pub fn topic(command: &str) -> Option<Topic> {
    docs().iter().find(|d| d.topic.command == command).map(|d| d.topic)
}

/// How a tool button's command relates to `command`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Runs {
    /// The command itself, or an alias of it (`u` for UNDO).
    Itself,
    /// One of its modes (`zoom.window`) or kinds (`arrayrect` for ARRAY).
    Mode,
}

fn runs_how(tool: &str, command: &str) -> Option<Runs> {
    if tool == command || (draftcraft_engine::find_command(tool).is_none() && draftcraft_engine::cmd::resolve_alias(tool) == command) {
        return Some(Runs::Itself);
    }
    let rest = tool.strip_prefix(command)?;
    // A kind of the command (`arrayrect`, `arraypolar`) when the command has no button of its own.
    let kind = !rest.is_empty() && draftcraft_engine::find_command(command).is_some_and(|s| s.menu.is_empty());
    (rest.starts_with('.') || kind).then_some(Runs::Mode)
}

/// Whether a tool button's command runs `command` (its id, an alias of it, or a mode of it).
fn runs(tool: &str, command: &str) -> bool {
    runs_how(tool, command).is_some()
}

/// Ribbon location for a command: (tab, panel) if found.
fn ribbon_location(command: &str) -> Option<(&'static str, &'static str)> {
    for tab in crate::ribbon::TABS {
        for panel in tab.panels {
            if crate::ribbon::all_tools(panel).into_iter().any(|(_, c, _)| c.eq_ignore_ascii_case(command)) {
                return Some((tab.name, panel.title));
            }
        }
    }
    None
}

/// The icon of a ribbon or Tool Sets button that runs `command` (its own before one of its modes').
pub fn tool_icon(command: &str) -> Option<Icon> {
    // Prefer the ribbon mapping first.
    if let Some(icon) = crate::ribbon::icon_for(command) {
        return Some(icon);
    }
    // Fall back to Tool Sets buttons.
    let sets = crate::palettes::DRAFTING.iter().chain(crate::palettes::MODELING).flat_map(|(_, large, small)| large.iter().chain(small.iter()));
    [Runs::Itself, Runs::Mode].into_iter().find_map(|how| sets.clone().find(|(_, c, _)| runs_how(c, command) == Some(how)).map(|(icon, _, _)| *icon))
}

/// Where to find `command`: what to type, the menu, the Tool Sets group, the ribbon, and the keyboard shortcut.
pub fn places(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let spec = draftcraft_engine::find_command(command);
    if !command.starts_with("ui.") {
        let aliases: Vec<String> =
            spec.map(|s| s.aliases).unwrap_or(&[]).iter().filter(|a| !a.starts_with('-')).take(3).map(|a| a.to_ascii_uppercase()).collect();
        let name = command.to_ascii_uppercase();
        out.push(if aliases.is_empty() {
            format!("Type {name} and press Enter")
        } else {
            format!("Type {name} (or {}) and press Enter", aliases.join(", "))
        });
    }
    // A command with no menu item whose twin has one (UNDO and U, both "Undo"): the twin's.
    let twin = spec
        .filter(|s| s.menu.is_empty())
        .and_then(|s| draftcraft_engine::command_specs().iter().find(|o| o.id != s.id && o.label == s.label && !o.menu.is_empty()));
    let on_screen = |c: &str| runs(c, command) || twin.is_some_and(|t| c == t.id);
    let menu: Vec<&str> = match spec.map(|s| s.menu).filter(|m| !m.is_empty()).or(twin.map(|t| t.menu)) {
        Some(m) => m.to_vec(),
        None => {
            // A command with no menu item of its own: the submenu its modes are in (View ▸ Zoom).
            let sub = draftcraft_engine::command_specs().iter().find(|s| s.id != command && runs(s.id, command) && s.menu.len() > 1);
            match sub {
                Some(s) => s.menu.get(..s.menu.len() - 1).unwrap_or(&[]).to_vec(),
                None => crate::menus::UI_COMMANDS.iter().find(|c| c.0 == command).map(|c| c.2.to_vec()).unwrap_or_default(),
            }
        }
    };
    if !menu.is_empty() {
        out.push(format!("Menu: {}", menu.join(" > ")));
    }
    for (tab, sets) in [("Drafting", crate::palettes::DRAFTING), ("Modeling", crate::palettes::MODELING)] {
        if let Some((group, _, _)) = sets.iter().find(|(_, large, small)| large.iter().chain(small.iter()).any(|(_, c, _)| runs(c, command))) {
            out.push(format!("Tool Sets: {tab} > {group}"));
            break;
        }
    }
    if let Some((tab, panel)) = ribbon_location(command) {
        out.push(format!("Ribbon: {tab} > {panel}"));
    }
    let key = spec
        .and_then(|s| s.shortcut)
        .or(twin.and_then(|t| t.shortcut))
        .or_else(|| crate::menus::UI_COMMANDS.iter().find(|c| c.0 == command).and_then(|c| c.3));
    if let Some(k) = key {
        out.push(format!("Keyboard: {k}"));
    }
    out
}

/// Whether a command-line entry is a question for Help rather than a command: it ends in `?`,
/// or starts with a question word ("how", "what") that is no command or alias.
pub fn is_question(text: &str) -> bool {
    let t = text.trim();
    if t.len() > 1 && t.ends_with('?') && !t.starts_with('(') {
        return true;
    }
    starts_question(t)
}

/// Whether `text` (the command line so far) starts with a question word, so a space types a
/// space instead of ending the entry.
pub fn starts_question(text: &str) -> bool {
    let Some(first) = text.split_whitespace().next() else { return false };
    let first = first.to_ascii_lowercase();
    let first = first.trim_end_matches('?');
    QUESTION_WORDS.contains(&first) && !known_name(first)
}

fn known_name(name: &str) -> bool {
    draftcraft_engine::find_command(name).is_some() || crate::menus::is_ui_command(name) || draftcraft_engine::cmd::resolve_alias(name) != name
}

/// Open the Help panel asking `question` (from the command line).
pub fn ask(app: &mut CadApp, question: &str) {
    let q = question.trim().to_string();
    if let Err(e) = open(app, Some(&q)) {
        app.session.echo(e);
        return;
    }
    let best = search(&q, 1).first().map(|h| h.topic.title);
    app.session.echo(match best {
        Some(t) => format!("Help: \"{q}\" (best match: {t})"),
        None => format!("Help: \"{q}\" (no match yet; try other words)"),
    });
}

/// `ui.panel.help` / HELP / F1: open the Help panel, with `{"query": "..."}` asking that.
pub fn command(app: &mut CadApp, params: &Value) -> Result<Value, String> {
    let query = params.get("query").and_then(Value::as_str);
    open(app, query)?;
    let hits: Vec<Value> = search(&app.ui.help.query, 5).iter().map(|h| json!({ "command": h.topic.command, "title": h.topic.title })).collect();
    Ok(json!({ "query": app.ui.help.query, "results": hits }))
}

/// Open (or bring forward) the Help panel, floating beside the drawing the first time.
pub fn open(app: &mut CadApp, query: Option<&str>) -> Result<Value, String> {
    if let Some(q) = query {
        app.ui.help.query = q.to_string();
        app.ui.help.selected = None;
    }
    app.ui.help.focus = true;
    let panel = crate::docking::Panel::Help;
    let first = !app.ui.docking.layout.contains(&panel) && !app.ui.docking.hidden.iter().any(|(p, _)| *p == panel);
    let r = crate::docking::command(app, false, &json!({ "operation": "open", "panel": panel.id() }))?;
    if !first {
        return Ok(r);
    }
    let area = app.canvas.rect.unwrap_or(egui::Rect::from_min_size(egui::pos2(220.0, 80.0), vec2(1000.0, 800.0)));
    let (w, h) = (360.0, (area.height() - 40.0).clamp(300.0, 680.0));
    let rect = [area.right() - w - 16.0, area.top() + 24.0, w, h];
    crate::docking::command(app, false, &json!({ "operation": "float", "panel": panel.id(), "rect": rect }))
}

/// An icon followed by a selectable title; returns whether it was clicked.
fn topic_row(ui: &mut egui::Ui, topic: &Topic, selected: bool) -> bool {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
        if let Some(icon) = topic.icon() {
            icons::paint(ui.painter(), rect, icon, false);
        }
        let label = format!("{}  ({})", crate::i18n::t(topic.title), topic.command.to_ascii_uppercase());
        ui.selectable_label(selected, label).clicked()
    })
    .inner
}

/// The full topic: icon, title, steps, where to find it, tip, related topics and Start.
fn detail(ui: &mut egui::Ui, topic: &Topic, select: &mut Option<&'static str>, start: &mut Option<&'static str>) {
    let t = Tokens::get();
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
        if let Some(icon) = topic.icon() {
            ui.painter().rect_stroke(rect, 4.0, egui::Stroke::new(1.0, t.text_dim), egui::StrokeKind::Inside);
            icons::paint(ui.painter(), rect.shrink(4.0), icon, false);
        }
        ui.vertical(|ui| {
            ui.label(RichText::new(crate::i18n::t(topic.title)).heading().color(t.text));
            ui.label(RichText::new(topic.command.to_ascii_uppercase()).small().color(t.text_dim));
        });
    });
    ui.add_space(4.0);
    // Start button early, just under the title, so it's visible without scrolling.
    let start_label = format!("Start {}", topic.command.to_ascii_uppercase());
    let start_resp = ui.button(start_label.clone()).on_hover_text("Close the question box and start this command");
    // Name the control for accessibility/testing like other buttons.
    crate::control::record_widget(ui, &start_resp, &start_label, "button", None);
    if start_resp.clicked() {
        *start = Some(topic.command);
    }
    ui.add_space(6.0);
    match topic.entry {
        Some(e) => {
            ui.label(e.summary);
            ui.add_space(6.0);
            ui.label(RichText::new("How to do it").strong());
            for (i, s) in e.steps.iter().enumerate() {
                ui.label(format!("{}. {s}", i + 1));
            }
        }
        None => {
            ui.label("No step-by-step help is written for this command yet. Start it and follow the prompts at the command line.");
        }
    }
    ui.add_space(6.0);
    ui.label(RichText::new("Where to find it").strong());
    for p in places(topic.command) {
        ui.label(format!("•  {p}"));
    }
    if let Some(e) = topic.entry {
        if !e.tip.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new(format!("Tip: {}", e.tip)).italics());
        }
        let related: Vec<Topic> = e.related.iter().filter_map(|c| self::topic(c)).collect();
        if !related.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new("See also").strong());
            ui.horizontal_wrapped(|ui| {
                for r in related {
                    let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                    if let Some(icon) = r.icon() {
                        icons::paint(ui.painter(), rect, icon, false);
                    }
                    if ui.link(crate::i18n::t(r.title)).clicked() {
                        *select = Some(r.command);
                    }
                }
            });
        }
    }
    // No second Start button at the bottom: having one at the top keeps tests deterministic.
}

/// The Help panel's body.
pub fn show(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let mut select: Option<&'static str> = None;
    let mut start: Option<&'static str> = None;
    let mut back = false;
    egui::Frame::NONE.inner_margin(8.0).show(ui, |ui| {
        let label = ui.label(RichText::new("Ask a question or type a command name").color(t.text_dim));
        let resp = ui.add(egui::TextEdit::singleline(&mut app.ui.help.query).hint_text("e.g. How do I draw a line?").desired_width(f32::INFINITY));
        let resp = resp.labelled_by(label.id);
        if std::mem::take(&mut app.ui.help.focus) {
            resp.request_focus();
        }
        if resp.changed() {
            app.ui.help.selected = None;
        }
        ui.add_space(6.0);
        let hits = search(&app.ui.help.query, 8);
        let shown = app.ui.help.selected.and_then(topic).or_else(|| hits.first().map(|h| h.topic));
        egui::ScrollArea::vertical().id_salt("help-body").auto_shrink([false, false]).show(ui, |ui| {
            if app.ui.help.query.trim().is_empty() && app.ui.help.selected.is_none() {
                ui.label("Type a question such as \"How do I draw a line?\", or pick a topic.");
                ui.add_space(4.0);
                for group in GROUPS {
                    ui.add_space(4.0);
                    ui.label(RichText::new(*group).strong());
                    for e in ENTRIES.iter().filter(|e| e.group == *group) {
                        let topic = Topic { command: e.command, title: e.title, entry: Some(e) };
                        if topic_row(ui, &topic, false) {
                            select = Some(e.command);
                        }
                    }
                }
                return;
            }
            if app.ui.help.query.trim().is_empty() && ui.link("< All topics").clicked() {
                back = true;
            }
            match shown {
                Some(topic) => detail(ui, &topic, &mut select, &mut start),
                None => {
                    ui.label("No matching help. Try other words, or type the command's name.");
                }
            }
            let others: Vec<&Hit> = hits.iter().filter(|h| Some(h.topic.command) != shown.map(|s| s.command)).collect();
            if !others.is_empty() {
                ui.add_space(10.0);
                ui.separator();
                ui.label(RichText::new("Other matches").strong());
                for h in others {
                    if topic_row(ui, &h.topic, false) {
                        select = Some(h.topic.command);
                    }
                }
            }
        });
    });
    if back {
        app.ui.help.selected = None;
    }
    if let Some(c) = select {
        app.ui.help.selected = Some(c);
    }
    if let Some(c) = start {
        // The command line takes the keys again.
        ui.ctx().memory_mut(|m| m.stop_text_input());
        app.start(c);
    }
}

#[cfg(test)]
mod tests;
