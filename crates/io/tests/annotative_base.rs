use draftcraft_doc::{AnnoScale, Common, Drawing, EntityKind, Space, Text};
use draftcraft_geom::Vec3;
use draftcraft_io::{read_dxf, write_dxf};

fn annotative_text(authored: f64) -> Drawing {
    let mut d = Drawing::new_metric();
    d.scales.push(AnnoScale::new("1:1250", 1.0, 1250.0));
    d.header.set_str("CANNOSCALE", "1:1250");
    let mut c = Common::default();
    c.anno_scales = vec![AnnoScale::new("1:1250", 1.0, 1250.0)];
    c.anno_authored = Some(authored);
    let t = Text { insert: Vec3::ZERO, align_pt: None, height: 2.5, value: "A".into(), rotation: 0.0, width_factor: 1.0, oblique: 0.0, style: "Standard".into(), halign: Default::default(), valign: Default::default() };
    d.add(&Space::Model, c, EntityKind::Text(t)).unwrap();
    d
}

#[test]
fn authored_scale_is_recorded_and_restored() {
    // Authored at 1:1250 → recorded in the annotation XRECORD.
    let d = annotative_text(1250.0);
    let text = write_dxf(&d);
    assert!(text.contains("DRAFTCRAFT_ANNOTATION"), "no annotation record written");
    // Read back: the object's authored factor is restored.
    let back = read_dxf(text.as_bytes()).unwrap();
    let e = back.model.iter().find(|e| matches!(e.kind, EntityKind::Text(_))).unwrap();
    assert_eq!(e.common.anno_authored, Some(1250.0));
}

