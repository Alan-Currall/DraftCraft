use egui::{vec2, ColorImage, TextureHandle};
use serde_json::json;

use crate::CadApp;

/// Attach Image dialog: pick a raster image and set insertion, scale and rotation with a live preview.
pub fn image_dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    // State: (path, at[x,y], scale, rotation, load error, preview texture)
    let id = egui::Id::unique("attach_image_state");
    let mut v = ctx
        .data_mut(|d| d.get_temp::<(String, [f64; 2], f64, f64, Option<String>, Option<TextureHandle>)>(id))
        .unwrap_or_else(|| (String::new(), [0.0, 0.0], 1.0, 0.0, None, None));
    let (mut apply, mut cancel) = (false, false);
    egui::Window::new(crate::tl!("Attach Image")).id(egui::Id::unique("attach_image")).open(open).resizable(false).show(ctx, |ui| {
        ui.horizontal(|ui| {
            let label = ui.label(crate::tl!("File"));
            let resp = ui.text_edit_singleline(&mut v.0).labelled_by(label.id);
            if resp.changed() {
                v.4 = None;
                v.5 = None;
            }
            if let Some(pick) = app.services.pick_file.as_ref() {
                if ui.button(crate::tl!("Browse…")).clicked() {
                    if let Some(path) = pick("Images (PNG, JPEG, BMP, TIFF)", &["png", "jpg", "jpeg", "bmp", "tif", "tiff"]) {
                        v.0 = path;
                        v.4 = None;
                        v.5 = None;
                    }
                }
            }
        });
        ui.horizontal(|ui| {
            let l = ui.label(crate::tl!("Insertion point (x, y)"));
            ui.add(egui::DragValue::new(&mut v.1[0]).speed(0.1)).labelled_by(l.id);
            ui.add(egui::DragValue::new(&mut v.1[1]).speed(0.1));
        });
        ui.horizontal(|ui| {
            let l = ui.label(crate::tl!("Scale (width in drawing units)"));
            ui.add(egui::DragValue::new(&mut v.2).speed(0.1).range(0.000001..=1e9)).labelled_by(l.id);
        });
        ui.horizontal(|ui| {
            let l = ui.label(crate::tl!("Rotation (degrees)"));
            ui.add(egui::DragValue::new(&mut v.3).speed(1.0)).labelled_by(l.id);
        });
        ui.separator();
        // Preview
        if v.5.is_none() && !v.0.trim().is_empty() && v.4.is_none() {
            if let Ok(bytes) = std::fs::read(&v.0) {
                if let Ok(img) = image::load_from_memory(&bytes) {
                    let rgba = img.to_rgba8();
                    let size = [rgba.width() as usize, rgba.height() as usize];
                    v.5 = Some(ui.ctx().load_texture("attach_image_preview", ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()), Default::default()));
                } else {
                    v.4 = Some(crate::tl!("Could not load the image file.").to_string());
                }
            } else {
                v.4 = Some(crate::tl!("File not found.").to_string());
            }
        }
        if let Some(tex) = &v.5 {
            let mut s = tex.size_vec2();
            let k = (200.0 / s.x).min(160.0 / s.y).min(1.0);
            s *= k;
            ui.label(crate::tl!("Preview"));
            ui.image((tex.id(), s));
        } else if let Some(err) = &v.4 {
            ui.label(err);
        }
        ui.separator();
        ui.horizontal(|ui| {
            apply = ui.button(crate::tl!("OK")).clicked();
            cancel = ui.button(crate::tl!("Cancel")).clicked();
        });
    });
    if apply && !v.0.trim().is_empty() {
        let _ = app.run("imageattach", json!({ "path": v.0, "at": v.1, "width": v.2, "rotation": v.3 }));
    }
    if apply || cancel || !*open {
        ctx.data_mut(|d| d.remove::<(String, [f64; 2], f64, f64, Option<String>, Option<TextureHandle>)>(id));
        *open = false;
    } else {
        ctx.data_mut(|d| d.insert_temp(id, v));
    }
}

/// Attach PDF Underlay dialog: pick a PDF, choose page, insertion, scale and rotation, with a page thumbnail.
pub fn pdf_dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let id = egui::Id::unique("attach_pdf_state");
    // (path, page (1-based), at[x,y], scale (units per inch), rotation, thumb texture, error)
    let mut v = ctx
        .data_mut(|d| d.get_temp::<(String, i32, [f64; 2], f64, f64, Option<TextureHandle>, Option<String>)>(id))
        .unwrap_or_else(|| (String::new(), 1, [0.0, 0.0], 25.4, 0.0, None, None));
    let (mut apply, mut cancel) = (false, false);
    egui::Window::new(crate::tl!("Attach PDF Underlay"))
        .id(egui::Id::unique("attach_pdf"))
        .open(open)
        .resizable(false)
        .default_size(vec2(420.0, 320.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let label = ui.label(crate::tl!("File"));
                let resp = ui.text_edit_singleline(&mut v.0).labelled_by(label.id);
                if resp.changed() {
                    v.5 = None;
                    v.6 = None;
                }
                if let Some(pick) = app.services.pick_file.as_ref() {
                    if ui.button(crate::tl!("Browse…")).clicked() {
                        if let Some(path) = pick("PDF files", &["pdf"]) {
                            v.0 = path;
                            v.5 = None;
                            v.6 = None;
                        }
                    }
                }
            });
            ui.horizontal(|ui| {
                let l = ui.label(crate::tl!("Page"));
                ui.add(egui::DragValue::new(&mut v.1).range(1..=9999).speed(1.0)).labelled_by(l.id);
            });
            ui.horizontal(|ui| {
                let l = ui.label(crate::tl!("Insertion point (x, y)"));
                ui.add(egui::DragValue::new(&mut v.2[0]).speed(0.1)).labelled_by(l.id);
                ui.add(egui::DragValue::new(&mut v.2[1]).speed(0.1));
            });
            ui.horizontal(|ui| {
                let l = ui.label(crate::tl!("Scale (drawing units per inch)"));
                ui.add(egui::DragValue::new(&mut v.3).speed(0.1).range(0.000001..=1e9)).labelled_by(l.id);
            });
            ui.horizontal(|ui| {
                let l = ui.label(crate::tl!("Rotation (degrees)"));
                ui.add(egui::DragValue::new(&mut v.4).speed(1.0)).labelled_by(l.id);
            });
            ui.separator();
            if v.5.is_none() && !v.0.trim().is_empty() && v.6.is_none() {
                let thumb = app
                    .session
                    .execute("pdfunderlay.thumb", &json!({ "path": v.0, "page": v.1, "maxSize": 256 }))
                    .ok()
                    .and_then(|r| {
                        let w = r["width"].as_u64()?;
                        let h = r["height"].as_u64()?;
                        let rgba = r["rgba"].as_str().and_then(draftcraft_engine::cmd::file::base64_decode)?;
                        Some((w as usize, h as usize, rgba))
                    });
                match thumb {
                    Some((w, h, rgba)) => {
                        v.5 = Some(ui.ctx().load_texture("attach_pdf_thumb", ColorImage::from_rgba_unmultiplied([w, h], &rgba), Default::default()));
                    }
                    None => v.6 = Some(crate::tl!("Could not render the PDF page.").to_string()),
                }
            }
            if let Some(tex) = &v.5 {
                let mut s = tex.size_vec2();
                let k = (220.0 / s.x).min(180.0 / s.y).min(1.0);
                s *= k;
                ui.label(crate::tl!("Page preview"));
                ui.image((tex.id(), s));
            } else if let Some(err) = &v.6 {
                ui.label(err);
            }
            ui.separator();
            ui.horizontal(|ui| {
                apply = ui.button(crate::tl!("OK")).clicked();
                cancel = ui.button(crate::tl!("Cancel")).clicked();
            });
        });
    if apply && !v.0.trim().is_empty() {
        let _ = app.run("pdfattach", json!({ "path": v.0, "page": v.1, "at": v.2, "scale": v.3, "rotation": v.4 }));
    }
    if apply || cancel || !*open {
        ctx.data_mut(|d| d.remove::<(String, i32, [f64; 2], f64, f64, Option<TextureHandle>, Option<String>)>(id));
        *open = false;
    } else {
        ctx.data_mut(|d| d.insert_temp(id, v));
    }
}

