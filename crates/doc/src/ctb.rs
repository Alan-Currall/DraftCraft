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

use crate::{PlotStyle, PlotStyleKind, PlotStyleTable};

/// Parse a `.ctb` (colour-dependent) or `.stb` (named) file from `bytes`.
pub fn parse_ctb_or_stb(name: &str, bytes: &[u8]) -> Result<PlotStyleTable, String> {
    let payload = decompress_first_zlib(bytes).map_err(|e| format!("not a CTB/STB (zlib payload not found or invalid): {e}"))?;
    // Try JSON: accept either a bare PlotStyleTable JSON or an object with a "table" field.
    // This lets tests feed a synthetic CTB/STB with a JSON body while we add fuller readers later.
    let text = std::str::from_utf8(&payload).map_err(|_| "decompressed payload is not UTF-8 text")?;
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
}

