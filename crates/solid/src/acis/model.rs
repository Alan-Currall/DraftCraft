//! ACIS records as DraftCraft writes them, and their two encodings: SAT text and SAB binary, save
//! format 700 (Spatial's published 7.0 save format, the level DWG and DXF readers take; docs/solids.md,
//! "The ACIS translator").
//!
//! A record is its name and its fields after the three every entity starts with (its attribute,
//! its history index and one more reference, all null here). SAT writes each field as a word;
//! SAB writes the same fields as tagged binary tokens, one tag byte and its little-endian value.

/// One field of a record.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Field {
    /// A reference to another record by its index; `None` is the null reference.
    Ref(Option<usize>),
    /// A real number.
    Real(f64),
    /// A position (lengths).
    Pos([f64; 3]),
    /// A vector (a direction, or an axis whose length counts).
    Vector([f64; 3]),
    /// A sense: forward (false) or reversed (true).
    Sense(bool),
    /// A face's sides: single.
    Single,
    /// A surface's v sense: forward.
    ForwardV,
    /// An unbounded parameter range end (SAT `I`).
    Infinite,
    /// A short string.
    Str(&'static str),
}

/// One record: its name (`plane-surface`, `coedge`…) and its fields.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Record {
    pub name: &'static str,
    pub fields: Vec<Field>,
}

/// The save format written.
pub(crate) const VERSION: u32 = 700;
/// What the header says wrote the data.
const PRODUCT: &str = "DraftCraft";
const ACIS_VERSION: &str = "ACIS 7.0";
/// The save date: fixed, so a solid saved twice keeps its bytes.
const DATE: &str = "Mon Jan 01 00:00:00 2001";
/// Millimetres per unit (the drawing's units are taken as they are), the absolute and the
/// normal resolution.
const SCALE: f64 = 1.0;
const RESABS: f64 = 1e-6;
const RESNOR: f64 = 1e-10;
/// The line (SAT) or name (SAB) that ends the data.
pub(crate) const END: &str = "End-of-ACIS-data";
/// What a binary ACIS stream starts with.
pub(crate) const SAB_MAGIC: &[u8; 15] = b"ACIS BinaryFile";

/// A real number as SAT writes it: the shortest text that reads back as the same number.
fn real(v: f64) -> String {
    let v = if v.is_finite() { v } else { 0.0 };
    // `-0` reads back as 0; write it as 0.
    if v == 0.0 { "0".into() } else { format!("{v}") }
}

fn triple(v: &[f64; 3]) -> String {
    format!("{} {} {}", real(v[0]), real(v[1]), real(v[2]))
}

/// The records as SAT text, header and end line included, one record a line.
pub(crate) fn sat(records: &[Record], bodies: usize) -> String {
    let mut o = format!("{VERSION} 0 {bodies} 0 \n");
    o.push_str(&format!("{} {} {} {} {} {} \n", PRODUCT.len(), PRODUCT, ACIS_VERSION.len(), ACIS_VERSION, DATE.len(), DATE));
    o.push_str(&format!("{} {} {} \n", real(SCALE), real(RESABS), real(RESNOR)));
    for r in records {
        o.push_str(r.name);
        o.push_str(" $-1 -1 $-1");
        for f in &r.fields {
            o.push(' ');
            match f {
                Field::Ref(Some(i)) => o.push_str(&format!("${i}")),
                Field::Ref(None) => o.push_str("$-1"),
                Field::Real(v) => o.push_str(&real(*v)),
                Field::Pos(p) | Field::Vector(p) => o.push_str(&triple(p)),
                Field::Sense(reversed) => o.push_str(if *reversed { "reversed" } else { "forward" }),
                Field::Single => o.push_str("single"),
                Field::ForwardV => o.push_str("forward_v"),
                Field::Infinite => o.push('I'),
                Field::Str(s) => o.push_str(&format!("@{} {s}", s.len())),
            }
        }
        o.push_str(" #\n");
    }
    o.push_str(END);
    o.push_str(" \n");
    o
}

/// SAB's tags.
mod tag {
    pub const INT: u8 = 0x04;
    pub const REAL: u8 = 0x06;
    pub const STR: u8 = 0x07;
    pub const TRUE: u8 = 0x0a;
    pub const FALSE: u8 = 0x0b;
    pub const REF: u8 = 0x0c;
    pub const IDENT: u8 = 0x0d;
    pub const SUB_IDENT: u8 = 0x0e;
    pub const END: u8 = 0x11;
    pub const POS: u8 = 0x13;
    pub const VECTOR: u8 = 0x14;
}

fn push_name(o: &mut Vec<u8>, t: u8, s: &str) {
    o.push(t);
    o.push(u8::try_from(s.len()).unwrap_or(u8::MAX));
    o.extend(s.bytes().take(usize::from(u8::MAX)));
}

/// A record name, its parts joined by `-`: every part but the last a sub-identifier.
fn push_record_name(o: &mut Vec<u8>, name: &str) {
    let parts: Vec<&str> = name.split('-').collect();
    for (i, part) in parts.iter().enumerate() {
        push_name(o, if i + 1 == parts.len() { tag::IDENT } else { tag::SUB_IDENT }, part);
    }
}

fn push_i32(o: &mut Vec<u8>, t: u8, v: i32) {
    o.push(t);
    o.extend_from_slice(&v.to_le_bytes());
}

fn push_real(o: &mut Vec<u8>, v: f64) {
    o.push(tag::REAL);
    o.extend_from_slice(&(if v.is_finite() { v } else { 0.0 }).to_le_bytes());
}

fn push_triple(o: &mut Vec<u8>, t: u8, v: &[f64; 3]) {
    o.push(t);
    for x in v {
        o.extend_from_slice(&(if x.is_finite() { *x } else { 0.0 }).to_le_bytes());
    }
}

fn reference(i: Option<usize>) -> i32 {
    i.and_then(|i| i32::try_from(i).ok()).unwrap_or(-1)
}

/// The records as SAB, header and end marker included.
pub(crate) fn sab(records: &[Record], bodies: usize) -> Vec<u8> {
    let mut o = Vec::new();
    o.extend_from_slice(SAB_MAGIC);
    let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    for v in [VERSION, 0, count(bodies), 0] {
        o.extend_from_slice(&v.to_le_bytes());
    }
    for s in [PRODUCT, ACIS_VERSION, DATE] {
        push_name(&mut o, tag::STR, s);
    }
    for v in [SCALE, RESABS, RESNOR] {
        push_real(&mut o, v);
    }
    for r in records {
        push_record_name(&mut o, r.name);
        push_i32(&mut o, tag::REF, -1);
        push_i32(&mut o, tag::INT, -1);
        push_i32(&mut o, tag::REF, -1);
        for f in &r.fields {
            match f {
                Field::Ref(i) => push_i32(&mut o, tag::REF, reference(*i)),
                Field::Real(v) => push_real(&mut o, *v),
                Field::Pos(p) => push_triple(&mut o, tag::POS, p),
                Field::Vector(p) => push_triple(&mut o, tag::VECTOR, p),
                Field::Sense(reversed) => o.push(if *reversed { tag::TRUE } else { tag::FALSE }),
                Field::Single | Field::ForwardV | Field::Infinite => o.push(tag::FALSE),
                Field::Str(s) => push_name(&mut o, tag::STR, s),
            }
        }
        o.push(tag::END);
    }
    push_record_name(&mut o, END);
    o
}

/// DXF and DWG store SAT text scrambled: each printable character `c` (`!` to `~`) as `159 - c`.
/// The same map undoes it.
pub fn scramble(text: &str) -> String {
    text.chars().map(|c| if ('!'..='~').contains(&c) { char::from(159 - c as u8) } else { c }).collect()
}
