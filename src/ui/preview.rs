use eframe::egui;

use crate::state::{AppState, Language};

pub fn show(ui: &mut egui::Ui, state: &mut AppState) {
    let Some(file) = state.selected().cloned() else {
        ui.centered_and_justified(|ui| {
            ui.weak("Chọn một file ở sidebar để xem trước.");
        });
        return;
    };

    if let Some(err) = &file.error {
        ui.colored_label(egui::Color32::from_rgb(220, 90, 90), err.to_string());
        return;
    }

    ui.horizontal(|ui| {
        if ui
            .selectable_label(state.show_diff, "🎨 Hiện diff")
            .on_hover_text(
                "So sánh Code gốc với bản Đã tag — đỏ = bị xoá, xanh = mới thêm. \
                 Mặc định tắt vì tính diff tốn thêm chi phí so với hiện text thường.",
            )
            .clicked()
        {
            state.show_diff = !state.show_diff;
        }

        // Bộ chọn ngôn ngữ chỉ có ý nghĩa khi KHÔNG ở chế độ diff — diff luôn
        // so Code gốc với pending_html (bản đã tag, chưa dịch), không phụ
        // thuộc ngôn ngữ đang xem.
        if !state.show_diff {
            ui.separator();
            if ui.selectable_label(state.preview_lang.is_none(), "Đã tag").clicked() {
                state.preview_lang = None;
            }
            for lang in Language::ALL {
                if ui
                    .selectable_label(state.preview_lang == Some(lang), lang.label())
                    .clicked()
                {
                    state.preview_lang = Some(lang);
                }
            }
        }
    });
    ui.add_space(4.0);

    let pending = file.pending_html.as_deref().unwrap_or("");

    let mut left_title = "Code gốc";
    let mut right_title = "Đã gắn tag (chưa dịch)".to_string();
    let mut right_content: &str = pending;
    let mut diff_jobs: Option<(egui::text::LayoutJob, egui::text::LayoutJob)> = None;

    if state.show_diff {
        left_title = "Code gốc (đỏ = bị xoá)";
        right_title = "Đã gắn tag (xanh = mới thêm)".to_string();
        diff_jobs = Some(crate::diff::build_diff_jobs(&file.content, pending));
    } else if let Some(lang) = state.preview_lang {
        right_title = format!("Bản dịch {}", lang.label());
        right_content = file
            .translated_by_lang
            .get(&lang)
            .map(String::as_str)
            .unwrap_or("(chưa có bản dịch — đang chờ API hoặc chưa bật ngôn ngữ này ở panel dưới)");
    }

    let spacing = 6.0;
    let total_width = ui.available_width();
    let left_width = ((total_width - spacing) * state.preview_split_ratio).max(80.0);
    let right_width = (total_width - spacing - left_width).max(80.0);
    let available_height = ui.available_height();

    ui.horizontal(|ui| {
        ui.allocate_ui(egui::vec2(left_width, available_height), |ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(left_title).strong());
                let job = diff_jobs.as_ref().map(|(left, _)| left.clone());
                editor_pane(ui, "left_pane", &file.content, job, &mut state.split_scroll.offset_y);
            });
        });

        // Splitter kéo tay: egui chưa có widget chia đôi dựng sẵn, nên tự làm
        // một handle mảnh, bắt Sense::drag() và cộng dồn drag_delta vào tỉ lệ chia.
        let (handle_rect, handle_resp) =
            ui.allocate_exact_size(egui::vec2(spacing, available_height), egui::Sense::drag());
        ui.painter().vline(
            handle_rect.center().x,
            handle_rect.top()..=handle_rect.bottom(),
            egui::Stroke::new(1.0, egui::Color32::GRAY),
        );
        if handle_resp.dragged() {
            state.preview_split_ratio =
                (state.preview_split_ratio + handle_resp.drag_delta().x / total_width).clamp(0.15, 0.85);
        }

        ui.allocate_ui(egui::vec2(right_width, available_height), |ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(right_title).strong());
                let job = diff_jobs.as_ref().map(|(_, right)| right.clone());
                editor_pane(ui, "right_pane", right_content, job, &mut state.split_scroll.offset_y);
            });
        });
    });
}

/// Pane chỉ đọc nhưng vẫn select/copy được, dùng trick chính thức của egui:
/// đưa `&mut &str` vào TextEdit thay vì `&mut String`. Nếu `colored` có giá
/// trị (chế độ diff), dùng `.layouter()` để tô màu theo LayoutJob đã tính sẵn
/// ở diff.rs thay vì để egui tự layout monospace mặc định — nội dung THỰC của
/// TextBuffer (`content`) và nội dung trong LayoutJob luôn khớp ký tự-với-ký-tự
/// (job được build từ đúng 2 chuỗi content/pending_html), nên cursor/selection
/// vẫn định vị đúng dù việc TÔ MÀU đến từ 1 nguồn khác (job) chứ không phải
/// buf mà layouter nhận vào.
///
/// Đồng bộ scroll: 2 pane dùng chung 1 biến offset trong AppState — xem giải
/// thích chi tiết ở bản gốc hàm này từ Phần 1.
fn editor_pane(
    ui: &mut egui::Ui,
    id_salt: &str,
    content: &str,
    colored: Option<egui::text::LayoutJob>,
    shared_offset: &mut f32,
) {
    let mut text = content;
    let output = egui::ScrollArea::vertical()
        .id_salt(id_salt)
        .auto_shrink([false, false])
        .scroll_offset(egui::vec2(0.0, *shared_offset))
        .show(ui, |ui| {
            if let Some(job) = colored {
                let mut layouter = move |ui: &egui::Ui, _buf: &dyn egui::TextBuffer, wrap_width: f32| {
                    let mut job = job.clone();
                    job.wrap.max_width = wrap_width;
                    ui.fonts_mut(|f| f.layout_job(job))
                };
                ui.add(
                    egui::TextEdit::multiline(&mut text)
                        .desired_width(f32::INFINITY)
                        .layouter(&mut layouter),
                );
            } else {
                ui.add(
                    egui::TextEdit::multiline(&mut text)
                        .code_editor()
                        .desired_width(f32::INFINITY),
                );
            }
        });

    if (output.state.offset.y - *shared_offset).abs() > 0.5 {
        *shared_offset = output.state.offset.y;
    }
}
