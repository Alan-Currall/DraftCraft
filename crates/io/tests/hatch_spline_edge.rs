use draftcraft_io::{read, write_dxf, write};

// A minimal DXF with one HATCH whose loop has one spline edge, then a closing line.
fn spline_hatch_dxf() -> String {
    // A cubic Bezier-like spline through (0,0) .. (4,0), peaking above.
    let spline = "\
0
SECTION
2
ENTITIES
0
HATCH
8
0
10
0
20
0
30
0
210
0
220
0
230
1
2
SOLID
70
1
71
0
91
1
92
1
93
2
72
4
94
3
73
0
74
0
95
8
96
4
40
0
40
0
40
0
40
0
40
1
40
1
40
1
40
1
10
0
20
0
10
0
20
4
10
4
20
4
10
4
20
0
97
0
72
1
10
4
20
0
11
0
21
0
97
0
75
0
76
1
98
0
0
ENDSEC
0
EOF
";
    spline.into()
}

#[test]
fn dxf_roundtrip_keeps_spline_edges() {
    let d = read(spline_hatch_dxf().as_bytes(), "h.dxf").unwrap();
    let out = write_dxf(&d);
    // Edge-based boundary with a spline edge (72=4), not converted to a polyline loop (92 has no bit 2).
    let has_spline = out.contains("\n72\n4\n") || out.contains("\r\n72\r\n4\r\n") || out.lines().collect::<Vec<_>>().windows(2).any(|w| w[0].trim() == "72" && w[1].trim() == "4");
    assert!(has_spline, "no spline edge written:\n{out}");
    // The loop header did not force a polyline boundary (2|outer): only the outer bit when edges exist.
    let has_ok_92 = out.lines().collect::<Vec<_>>().windows(2).any(|w| w[0].trim() == "92" && (w[1].trim() == "1" || w[1].trim() == "0"));
    assert!(has_ok_92, "edge-boundary 92 flags should not carry bit 2:\n{out}");
}

#[test]
fn dwg_write_keeps_spline_edges() {
    let d = read(spline_hatch_dxf().as_bytes(), "h.dxf").unwrap();
    let dwg = write(&d, "x.dwg").unwrap();
    // Read back the DWG and dump as DXF, which the DWG writer derives from our DXF tags.
    let back = draftcraft_io::read(&dwg, "x.dwg").unwrap();
    let text = write_dxf(&back);
    let has = text.contains("\n72\n4\n") || text.contains("\r\n72\r\n4\r\n") || text.lines().collect::<Vec<_>>().windows(2).any(|w| w[0].trim() == "72" && w[1].trim() == "4");
    assert!(has, "DWG path lost spline edge:\n{text}");
}

