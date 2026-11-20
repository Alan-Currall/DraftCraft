use egui::{vec2, ColorImage, TextureHandle};
use serde_json::json;

use crate::CadApp;

pub fn image_dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let id = egui::Id::unique("attach_image_state");
    #[derive(Clone, Copy, PartialEq)]
    enum PathType {
        Full,
        Relative,
        NonePath,
    }
    let mut v = ctx
        .data_mut(|d| d.get_temp::<(String, [f64; 2], f64, f64, f64, PathType, bool, bool, bool, Option<String>, Option<TextureHandle>)>(id))
        .unwrap_or_else(|| (String::new(), [0.0, 0.0], 1.0, 0.0, 0.0, PathType::Full, false, false, false, None, None));
    let (mut apply, mut cancel) = (false, false);
    egui::Window::new(crate::tl!("Attach Image")).id(egui::Id::unique("attach_image")).open(open).resizable(false).show(ctx, |ui| {
        ui.horizontal(|ui| {
            let label = ui.label(crate::tl!("File"));
            let resp = ui.text_edit_singleline(&mut v.0).labelled_by(label.id);
            if resp.changed() {
                v.9 = None;
                v.10 = None;
            }
            if let Some(pick) = app.services.pick_file.as_ref() {
                if ui.button(crate::tl!("Browse…")).clicked() {
                    if let Some(path) = pick("Images (PNG, JPEG, BMP, TIFF)", &["png", "jpg", "jpeg", "bmp", "tif", "tiff"]) {
                        v.0 = path;
                        v.9 = None;
                        v.10 = None;
                    }
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(crate::tl!("Path type"));
            egui::ComboBox::from_id_salt("img_path_type")
                .selected_text(match v.5 {
                    PathType::Full => crate::tl!("Full path"),
                    PathType::Relative => crate::tl!("Relative path"),
                    PathType::NonePath => crate::tl!("No path"),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut v.5, PathType::Full, crate::tl!("Full path"));
                    ui.selectable_value(&mut v.5, PathType::Relative, crate::tl!("Relative path"));
                    ui.selectable_value(&mut v.5, PathType::NonePath, crate::tl!("No path"));
                });
        });
        ui.horizontal(|ui| {
            let l = ui.label(crate::tl!("Insertion point (x, y)"));
            ui.add(egui::DragValue::new(&mut v.1[0]).speed(0.1)).labelled_by(l.id);
            ui.add(egui::DragValue::new(&mut v.1[1]).speed(0.1));
            ui.checkbox(&mut v.6, crate::tl!("Specify on screen"));
        });
        ui.horizontal(|ui| {
            let l = ui.label(crate::tl!("Width (drawing units)"));
            ui.add(egui::DragValue::new(&mut v.2).speed(0.1).range(0.000001..=1e9)).labelled_by(l.id);
        });
        ui.horizontal(|ui| {
            let l = ui.label(crate::tl!("Scale (image width multiplier)"));
            ui.add(egui::DragValue::new(&mut v.4).speed(0.1).range(0.000001..=1e9)).labelled_by(l.id);
            ui.checkbox(&mut v.7, crate::tl!("Specify on screen"));
        });
        ui.horizontal(|ui| {
            let l = ui.label(crate::tl!("Rotation (degrees)"));
            ui.add(egui::DragValue::new(&mut v.3).speed(1.0)).labelled_by(l.id);
            ui.checkbox(&mut v.8, crate::tl!("Specify on screen"));
        });
        ui.separator();
        if v.10.is_none() && !v.0.trim().is_empty() && v.9.is_none() {
            if let Ok(bytes) = std::fs::read(&v.0) {
                if let Ok(img) = image::load_from_memory(&bytes) {
                    let rgba = img.to_rgba8();
                    let size = [rgba.width() as usize, rgba.height() as usize];
                    v.10 = Some(ui.ctx().load_texture("attach_image_preview", ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()), Default::default()));
                } else {
                    v.9 = Some(crate::tl!("Could not load the image file.").to_string());
                }
            } else {
                v.9 = Some(crate::tl!("File not found.").to_string());
            }
        }
        if let Some(tex) = &v.10 {
            let mut s = tex.size_vec2();
            let k = (200.0 / s.x).min(160.0 / s.y).min(1.0);
            s *= k;
            ui.label(crate::tl!("Preview"));
            ui.image((tex.id(), s));
        } else if let Some(err) = &v.9 {
            ui.label(err);
        }
        ui.separator();
        ui.horizontal(|ui| {
            apply = ui.button(crate::tl!("OK")).clicked();
            cancel = ui.button(crate::tl!("Cancel")).clicked();
        });
    });
    if apply && !v.0.trim().is_empty() {
        let path_type = match v.5 {
            PathType::Full => "full",
            PathType::Relative => "relative",
            PathType::NonePath => "none",
        };
        let specify_any = v.6 || v.7 || v.8;
        if specify_any {
            *open = false;
            app.start("imageattach");
            app.cmdline(&v.0);
            app.cmdline(match path_type {
                "full" => "Full",
                "relative" => "Relative",
                _ => "None",
            });
            if !v.6 {
                app.cmdline(&format!("{},{}", v.1[0], v.1[1]));
            }
            if !v.7 {
                if v.4 > 0.0 {
                    app.cmdline(&format!("{}", v.4));
                } else {
                    app.cmdline(&format!("{}", v.2));
                }
            }
            if !v.8 {
                app.cmdline(&format!("{}", v.3));
            }
        } else {
            let mut params = json!({ "path": v.0, "pathType": path_type, "rotation": v.3, "at": v.1 });
            if v.4 > 0.0 {
                params["scale"] = json!(v.4);
            } else {
                params["width"] = json!(v.2);
            }
            let _ = app.run("imageattach", params);
        }
    }
    if apply || cancel || !*open {
        ctx.data_mut(|d| d.remove::<(String, [f64; 2], f64, f64, f64, PathType, bool, bool, bool, Option<String>, Option<TextureHandle>)>(id));
        *open = false;
    } else {
        ctx.data_mut(|d| d.insert_temp(id, v));
    }
}

pub fn pdf_dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let id = egui::Id::unique("attach_pdf_state");
    #[derive(Clone, Copy, PartialEq)]
    enum PathTypePdf {
        Full,
        Relative,
        NonePath,
    }
    let mut v = ctx
        .data_mut(|d| d.get_temp::<(String, i32, [f64; 2], f64, f64, PathTypePdf, bool, bool, bool, Option<Vec<Option<TextureHandle>>>, Option<String>)>(id))
        .unwrap_or_else(|| (String::new(), 1, [0.0, 0.0], 25.4, 0.0, PathTypePdf::Full, false, false, false, None, None));
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
                    v.9 = None;
                    v.10 = None;
                }
                if let Some(pick) = app.services.pick_file.as_ref() {
                    if ui.button(crate::tl!("Browse…")).clicked() {
                        if let Some(path) = pick("PDF files", &["pdf"]) {
                            v.0 = path;
                            v.9 = None;
                            v.10 = None;
                        }
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label(crate::tl!("Path type"));
                egui::ComboBox::from_id_salt("pdf_path_type")
                    .selected_text(match v.5 {
                        PathTypePdf::Full => crate::tl!("Full path"),
                        PathTypePdf::Relative => crate::tl!("Relative path"),
                        PathTypePdf::NonePath => crate::tl!("No path"),
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut v.5, PathTypePdf::Full, crate::tl!("Full path"));
                        ui.selectable_value(&mut v.5, PathTypePdf::Relative, crate::tl!("Relative path"));
                        ui.selectable_value(&mut v.5, PathTypePdf::NonePath, crate::tl!("No path"));
                    });
            });
            ui.horizontal(|ui| {
                let l = ui.label(crate::tl!("Page"));
                ui.add(egui::DragValue::new(&mut v.1).range(1..=9999).speed(1.0)).labelled_by(l.id);
            });
            ui.horizontal(|ui| {
                let l = ui.label(crate::tl!("Insertion point (x, y)"));
                ui.add(egui::DragValue::new(&mut v.2[0]).speed(0.1)).labelled_by(l.id);
                ui.add(egui::DragValue::new(&mut v.2[1]).speed(0.1));
                ui.checkbox(&mut v.6, crate::tl!("Specify on screen"));
            });
            ui.horizontal(|ui| {
                let l = ui.label(crate::tl!("Scale (drawing units per inch)"));
                ui.add(egui::DragValue::new(&mut v.3).speed(0.1).range(0.000001..=1e9)).labelled_by(l.id);
                // reuse v.7 as scale specify
                ui.checkbox(&mut v.7, crate::tl!("Specify on screen"));
            });
            ui.horizontal(|ui| {
                let l = ui.label(crate::tl!("Rotation (degrees)"));
                ui.add(egui::DragValue::new(&mut v.4).speed(1.0)).labelled_by(l.id);
                ui.checkbox(&mut v.8, crate::tl!("Specify on screen"));
            });
            ui.separator();
            // Thumbnail panel for pages
            if !v.0.trim().is_empty() {
                if v.9.is_none() {
                    // Fetch page count
                    if let Ok(r) = app.session.execute("pdfunderlay.pages", &json!({ "path": v.0 })) {
                        let pages = r["pages"].as_u64().unwrap_or(0) as usize;
                        if pages > 0 && pages <= 500 {
                            v.9 = Some(vec![None; pages]);
                        } else {
                            v.10 = Some(crate::tl!("Could not read PDF pages.").to_string());
                        }
                    } else {
                        v.10 = Some(crate::tl!("Could not read PDF pages.").to_string());
                    }
                }
                if let Some(grid) = &mut v.9 {
                    ui.label(crate::tl!("Pages"));
                    egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                        let cols = 4;
                        for row in 0..((grid.len() + cols - 1) / cols) {
                            ui.horizontal(|ui| {
                                for col in 0..cols {
                                    let idx = row * cols + col;
                                    if idx >= grid.len() {
                                        break;
                                    }
                                    if grid[idx].is_none() {
                                        if let Ok(r) = app.session.execute("pdfunderlay.thumb", &json!({ "path": v.0, "page": (idx as i32) + 1, "maxSize": 120 })) {
                                            if let (Some(w), Some(h), Some(rgba)) = (
                                                r["width"].as_u64(),
                                                r["height"].as_u64(),
                                                r["rgba"].as_str().and_then(draftcraft_engine::cmd::file::base64_decode),
                                            ) {
                                                grid[idx] = Some(ui.ctx().load_texture(
                                                    format!("attach_pdf_thumb_{idx}"),
                                                    ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba),
                                                    Default::default(),
                                                ));
                                            }
                                        }
                                    }
                                    if let Some(tex) = &grid[idx] {
                                        let mut s = tex.size_vec2();
                                        let k = (100.0 / s.x).min(100.0 / s.y).min(1.0);
                                        s *= k;
                                        if ui.add(egui::Image::new((tex.id(), s))).clicked() {
                                            v.1 = (idx as i32) + 1;
                                        }
                                    } else {
                                        ui.allocate_ui(vec2(100.0, 100.0), |_| {});
                                    }
                                }
                            });
                        }
                    });
                } else if let Some(err) = &v.10 {
                    ui.label(err);
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                apply = ui.button(crate::tl!("OK")).clicked();
                cancel = ui.button(crate::tl!("Cancel")).clicked();
            });
        });
    if apply && !v.0.trim().is_empty() {
        let path_type = match v.5 {
            PathTypePdf::Full => "full",
            PathTypePdf::Relative => "relative",
            PathTypePdf::NonePath => "none",
        };
        let specify_any = v.6 || v.7 || v.8;
        if specify_any {
            *open = false;
            app.start("pdfattach");
            app.cmdline(&v.0);
            app.cmdline(match path_type {
                "full" => "Full",
                "relative" => "Relative",
                _ => "None",
            });
            app.cmdline(&v.1.to_string());
            if !v.6 {
                app.cmdline(&format!("{},{}", v.2[0], v.2[1]));
            }
            if !v.7 {
                app.cmdline(&format!("{}", v.3));
            }
            if !v.8 {
                app.cmdline(&format!("{}", v.4));
            }
        } else {
            let mut params = json!({ "path": v.0, "pathType": path_type, "page": v.1, "scale": v.3, "rotation": v.4, "at": v.2 });
            let _ = app.run("pdfattach", params);
        }
    }
    if apply || cancel || !*open {
        ctx.data_mut(|d| d.remove::<(String, i32, [f64; 2], f64, f64, PathTypePdf, bool, bool, bool, Option<Vec<Option<TextureHandle>>>, Option<String>)>(id));
        *open = false;
    } else {
        ctx.data_mut(|d| d.insert_temp(id, v));
    }
}

