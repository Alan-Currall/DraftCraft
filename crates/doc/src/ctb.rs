//! CTB/STB reader (best-effort).
//!
//! AutoCAD plot style tables (`.ctb` colour-dependent, `.stb` named) are binary files that
//! carry a short ASCII header followed by a zlib-compressed text payload. The exact text
//! format varies by version and vendor. For v1 we implement a conservative reader:
//! - Detect the first zlib stream in the file and decompress it.
//! - Try to parse the payload as JSON compatible with `PlotStyleTable` (our own schema).
//! - Otherwise, return an error.
//!
//! This is intentionally strict and read-only. It never panics; unknown formats return
//! an error message.

use crate::{EndStyle, FillStyle, JoinStyle, PlotStyle, PlotStyleKind, PlotStyleTable};

/// Parse a `.ctb` (colour-dependent) or `.stb` (named) file from `bytes`.
pub fn parse_ctb_or_stb(name: &str, bytes: &[u8]) -> Result<PlotStyleTable, String> {
    let payload = decompress_first_zlib(bytes).map_err(|e| format!("not a CTB/STB (zlib payload not found or invalid): {e}"))?;
    // First try the real plaintext key=value format with nested brace blocks.
    let text = std::str::from_utf8(&payload).map_err(|_| "decompressed payload is not UTF-8 text")?;
    if let Ok(t) = parse_ctb_stb_text(name, text) {
        return Ok(t);
    }
    // Fallback: accept a bare PlotStyleTable JSON or an object with a "table" field.
    // 1) Plain table JSON
    if let Ok(mut t) = serde_json::from_str::<PlotStyleTable>(text) {
        if t.name.trim().is_empty() {
            t.name = name.to_string();
        }
        return Ok(t.sanitized());
    }
    // 2) Wrapped in {"table": {..}}
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
        if let Some(obj) = v.get("table") {
            if let Ok(mut t) = serde_json::from_value::<PlotStyleTable>(obj.clone()) {
                if t.name.trim().is_empty() {
                    t.name = name.to_string();
                }
                return Ok(t.sanitized());
            }
        }
        // 3) A minimal color-to-lineweight mapping: {"kind":"ctb","lineweights":{"1":50,"2":35,...}}
        if let Some(kind) = v.get("kind").and_then(|k| k.as_str()) {
            if kind.eq_ignore_ascii_case("ctb") {
                if let Some(map) = v.get("lineweights").and_then(|m| m.as_object()) {
                    let mut styles: Vec<PlotStyle> = Vec::with_capacity(crate::CTB_STYLES);
                    for i in 1..=255u8 {
                        let key = i.to_string();
                        let lw = map.get(&key).and_then(|n| n.as_f64()).map(|mm| (mm * 100.0).round() as u16);
                        let mut st = PlotStyle::object(&format!("Color_{i}"));
                        st.lineweight = lw;
                        styles.push(st);
                    }
                    return Ok(PlotStyleTable { name: name.to_string(), description: String::new(), kind: PlotStyleKind::ColorDependent, styles }.sanitized());
                }
            } else if kind.eq_ignore_ascii_case("stb") {
                if let Some(arr) = v.get("styles").and_then(|s| s.as_array()) {
                    let mut styles: Vec<PlotStyle> = Vec::new();
                    for it in arr {
                        if let Ok(mut st) = serde_json::from_value::<PlotStyle>(it.clone()) {
                            if st.name.trim().is_empty() {
                                continue;
                            }
                            styles.push(st);
                        }
                    }
                    return Ok(PlotStyleTable::named(name, "", styles));
                }
            }
        }
    }
    Err("unsupported CTB/STB payload (not JSON)".into())
}

/// Parse the AutoCAD plaintext CTB/STB payload: key=value lines with nested blocks in `{}`.
fn parse_ctb_stb_text(name: &str, text: &str) -> Result<PlotStyleTable, String> {
    let mut p = KeyBlockParser::new(text);
    let root = p.parse().map_err(|e| format!("ctb text: {e}"))?;
    let lower_ext = name.to_ascii_lowercase();
    let is_ctb = lower_ext.ends_with(".ctb");
    let desc = root.get("description").unwrap_or_default();
    let styles = root.child("plot_style").ok_or_else(|| "no plot_style block".to_string())?;
    if is_ctb {
        // Build Color_1..Color_255; apply overrides from plot_style{ i{ ... } } if present.
        let mut out: Vec<PlotStyle> = (1..=255u16).map(|i| PlotStyle::object(&format!("Color_{i}"))).collect();
        for (k, v) in &styles.objects {
            let i = k.parse::<usize>().ok().filter(|i| (1..=255).contains(i)).unwrap_or(0);
            if i == 0 {
                continue;
            }
            if let Some(st) = out.get_mut(i - 1) {
                apply_fields(st, v);
            }
        }
        Ok(PlotStyleTable { name: name.to_string(), description: desc, kind: PlotStyleKind::ColorDependent, styles: out }.sanitized())
    } else {
        // Named plot styles from entries under plot_style.
        let mut out: Vec<PlotStyle> = Vec::new();
        for (_k, v) in &styles.objects {
            let name = v.get("name").filter(|n| !n.is_empty()).unwrap_or_else(|| "Style".to_string());
            let mut s = PlotStyle::object(&name);
            s.description = v.get("description").unwrap_or_default();
            apply_fields(&mut s, v);
            out.push(s);
        }
        Ok(PlotStyleTable::named(name, desc.as_str(), out))
    }
}

fn apply_fields(s: &mut PlotStyle, v: &KeyBlock) {
    // color_policy: 0 keep object; 1 monochrome (black); 2 grayscale
    if let Some(cp) = v.get("color_policy").and_then(|t| t.parse::<i32>().ok()) {
        match cp {
            1 => s.color = Some(crate::color::Rgb(0, 0, 0)),
            2 => s.grayscale = true,
            _ => {}
        }
    }
    if let Some(c) = v.get("color") {
        if let Some(rgb) = parse_rgb_triplet(&c).map(|(r, g, b)| crate::color::Rgb(r, g, b)).or_else(|| crate::color::Rgb::parse_hex(c.trim().trim_start_matches('#'))) {
            s.color = Some(rgb);
        }
    }
    if let Some(screen) = v.get("screen").and_then(|t| t.parse::<i32>().ok()) {
        s.screening = screen.clamp(0, 100) as u8;
    }
    if let Some(lt) = v.get("linetype").filter(|t| !t.eq_ignore_ascii_case("object") && !t.trim().is_empty()) {
        s.linetype = Some(lt.to_string());
    }
    if let Some(w) = v.get("lineweight").and_then(|t| t.parse::<f64>().ok()) {
        if w.is_finite() && w >= 0.0 {
            s.lineweight = Some((w * 100.0).round() as u16);
        }
    }
    if let Some(e) = v.get("end_style") {
        s.end = match e.to_ascii_lowercase().as_str() {
            "butt" => EndStyle::Butt,
            "square" => EndStyle::Square,
            "round" => EndStyle::Round,
            "diamond" => EndStyle::Diamond,
            _ => EndStyle::Object,
        };
    }
    if let Some(j) = v.get("join_style") {
        s.join = match j.to_ascii_lowercase().as_str() {
            "miter" => JoinStyle::Miter,
            "bevel" => JoinStyle::Bevel,
            "round" => JoinStyle::Round,
            "diamond" => JoinStyle::Diamond,
            _ => JoinStyle::Object,
        };
    }
    if let Some(f) = v.get("fill_style") {
        s.fill = match f.to_ascii_lowercase().as_str() {
            "solid" => FillStyle::Solid,
            "checkerboard" => FillStyle::Checkerboard,
            "crosshatch" => FillStyle::Crosshatch,
            "diamonds" => FillStyle::Diamonds,
            "horizontalbars" => FillStyle::HorizontalBars,
            "slantleft" => FillStyle::SlantLeft,
            "slantright" => FillStyle::SlantRight,
            "squaredots" => FillStyle::SquareDots,
            "verticalbars" => FillStyle::VerticalBars,
            _ => FillStyle::Object,
        };
    }
}

fn parse_rgb_triplet(t: &str) -> Option<(u8, u8, u8)> {
    let parts: Vec<&str> = t.split(',').map(str::trim).collect();
    if parts.len() != 3 {
        return None;
    }
    let r = parts[0].parse::<u16>().ok()?;
    let g = parts[1].parse::<u16>().ok()?;
    let b = parts[2].parse::<u16>().ok()?;
    if r <= 255 && g <= 255 && b <= 255 { Some((r as u8, g as u8, b as u8)) } else { None }
}

#[derive(Clone, Debug, Default)]
struct KeyBlock {
    scalars: std::collections::HashMap<String, String>,
    objects: std::collections::HashMap<String, KeyBlock>,
}
impl KeyBlock {
    fn get(&self, k: &str) -> Option<String> {
        self.scalars.get(&k.to_ascii_lowercase()).cloned()
    }
    fn child(&self, k: &str) -> Option<&KeyBlock> {
        self.objects.get(&k.to_ascii_lowercase())
    }
}

struct KeyBlockParser<'a> {
    s: &'a [u8],
    i: usize,
}
impl<'a> KeyBlockParser<'a> {
    fn new(text: &'a str) -> Self {
        Self { s: text.as_bytes(), i: 0 }
    }
    fn parse(&mut self) -> Result<KeyBlock, String> {
        self.skip_ws();
        let mut root = KeyBlock::default();
        // Top-level is a sequence of key=value or key{...}
        while self.i < self.s.len() {
            self.skip_ws();
            if self.i >= self.s.len() {
                break;
            }
            let key = self.read_key();
            if key.is_empty() {
                self.consume_line();
                continue;
            }
            self.skip_ws();
            match self.peek_char() {
                Some('{') => {
                    self.i += 1;
                    let child = self.parse_block()?;
                    root.objects.insert(key.to_ascii_lowercase(), child);
                }
                Some('=') => {
                    self.i += 1;
                    let val = self.read_value();
                    root.scalars.insert(key.to_ascii_lowercase(), val);
                }
                _ => self.consume_line(),
            }
        }
        Ok(root)
    }
    fn parse_block(&mut self) -> Result<KeyBlock, String> {
        let mut b = KeyBlock::default();
        loop {
            self.skip_ws();
            if self.peek_char() == Some('}') {
                self.i += 1;
                break;
            }
            if self.i >= self.s.len() {
                break;
            }
            let key = self.read_key();
            if key.is_empty() {
                self.consume_line();
                continue;
            }
            self.skip_ws();
            match self.peek_char() {
                Some('{') => {
                    self.i += 1;
                    let child = self.parse_block()?;
                    b.objects.insert(key.to_ascii_lowercase(), child);
                }
                Some('=') => {
                    self.i += 1;
                    let val = self.read_value();
                    b.scalars.insert(key.to_ascii_lowercase(), val);
                }
                Some('}') => {
                    // Empty entry, consume and end
                    self.i += 1;
                    break;
                }
                _ => self.consume_line(),
            }
        }
        Ok(b)
    }
    fn read_key(&mut self) -> String {
        let start = self.i;
        while self.i < self.s.len() {
            let c = self.s[self.i] as char;
            if c.is_whitespace() || c == '=' || c == '{' || c == '}' {
                break;
            }
            self.i += 1;
        }
        String::from_utf8_lossy(&self.s[start..self.i]).trim().to_string()
    }
    fn read_value(&mut self) -> String {
        let start = self.i;
        while self.i < self.s.len() {
            let c = self.s[self.i] as char;
            if c == '\n' || c == '\r' || c == '}' {
                break;
            }
            self.i += 1;
        }
        let v = String::from_utf8_lossy(&self.s[start..self.i]).to_string();
        // Leave '}' for the caller if we stopped there.
        if self.peek_char() != Some('}') {
            self.consume_line();
        }
        v.trim().trim_matches('"').to_string()
    }
    fn consume_line(&mut self) {
        while self.i < self.s.len() && !matches!(self.s[self.i] as char, '\n' | '\r') {
            self.i += 1;
        }
        while self.i < self.s.len() && matches!(self.s[self.i] as char, '\n' | '\r') {
            self.i += 1;
        }
    }
    fn skip_ws(&mut self) {
        while self.i < self.s.len() && (self.s[self.i] as char).is_whitespace() {
            self.i += 1;
        }
    }
    fn peek_char(&self) -> Option<char> {
        (self.i < self.s.len()).then_some(self.s[self.i] as char)
    }
}

fn decompress_first_zlib(bytes: &[u8]) -> Result<Vec<u8>, String> {
    // Look for a zlib header: CMF/FLG where CM = 8 (deflate) and (CMF*256 + FLG) % 31 == 0.
    for i in 0..bytes.len().saturating_sub(2) {
        let cmf = bytes[i];
        let flg = bytes[i + 1];
        if (cmf & 0x0F) == 8 && ((u16::from(cmf) << 8 | u16::from(flg)) % 31 == 0) {
            // Best-effort: try inflating from here.
            if let Ok(out) = miniz_oxide::inflate::decompress_to_vec_zlib(&bytes[i..]) {
                if !out.is_empty() {
                    return Ok(out);
                }
            }
        }
    }
    Err("no zlib stream found".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ctb_bytes(body: &str) -> Vec<u8> {
        let mut out = Vec::from(b"PIAFILEVERSION_2.0\n"[..].to_vec());
        let mut comp = miniz_oxide::deflate::compress_to_vec_zlib(body.as_bytes(), 6);
        out.append(&mut comp);
        out
    }

    #[test]
    fn json_payload_round_trips() {
        let t = PlotStyleTable::named(
            "pens.stb",
            "office pens",
            vec![PlotStyle { name: "Thick".into(), lineweight: Some(70), ..PlotStyle::default() }],
        );
        let bytes = make_ctb_bytes(&serde_json::to_string(&t).unwrap());
        let got = parse_ctb_or_stb("pens.stb", &bytes).unwrap();
        assert_eq!(got, t);
    }

    #[test]
    fn minimal_lineweight_map_builds_a_ctb() {
        let body = r#"{"kind":"ctb","lineweights":{"1":0.50,"2":0.35,"255":0.13}}"#;
        let bytes = make_ctb_bytes(body);
        let got = parse_ctb_or_stb("map.ctb", &bytes).unwrap();
        assert!(got.is_color_dependent());
        assert_eq!(got.by_color(1).unwrap().lineweight, Some(50));
        assert_eq!(got.by_color(2).unwrap().lineweight, Some(35));
        assert_eq!(got.by_color(255).unwrap().lineweight, Some(13));
    }

    #[test]
    fn plaintext_ctb_monochrome_and_screening_parse() {
        let body = r#"
description=Demo
plot_style{
 1{
  name=Color_1
  color_policy=1
  screen=50
  lineweight=0.50
 }
}
"#;
        let bytes = make_ctb_bytes(body);
        let t = parse_ctb_or_stb("demo.ctb", &bytes).unwrap();
        assert!(t.is_color_dependent());
        let s = t.by_color(1).unwrap();
        assert_eq!(s.color, Some(crate::color::Rgb(0, 0, 0)));
        assert_eq!(s.screening, 50);
        assert_eq!(s.lineweight, Some(50));
    }
}

