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

    let (left_title, right_title, right_raw): (&str, String, &str) = if state.show_diff {
        ("Code gốc (đỏ = bị xoá)", "Đã gắn tag (xanh = mới thêm)".to_string(), pending)
    } else if let Some(lang) = state.preview_lang {
        (
            "Code gốc",
            format!("Bản dịch {}", lang.label()),
            file.translated_by_lang
                .get(&lang)
                .map(String::as_str)
                .unwrap_or("(chưa có bản dịch — đang chờ API hoặc chưa bật ngôn ngữ này ở panel dưới)"),
        )
    } else {
        ("Code gốc", "Đã gắn tag (chưa dịch)".to_string(), pending)
    };

    // LUÔN dựng job đã GIÓNG HÀNG cho cả 2 vế (xem diff.rs) — chỉ khác nhau
    // ở việc có tô đỏ/xanh hay không. Trước đây chỉ chế độ "Hiện diff" mới
    // gióng hàng, còn Đã tag/EN/VI/ZH/JA hiện thẳng text thô — nhưng những
    // cặp đó CŨNG lệch dòng (html5ever tổ chức lại xuống dòng khi parse rồi
    // serialize lại, không chỉ do gắn thêm attribute), nên giờ áp dụng đều.
    let (left_job, right_job) = if state.show_diff {
        crate::diff::build_diff_jobs(&file.content, right_raw)
    } else {
        crate::diff::build_aligned_plain_jobs(&file.content, right_raw)
    };

    // Dùng CHÍNH text đã gióng hàng (job.text — có thể dài hơn nội dung gốc
    // do được chèn thêm dòng trống bù) làm buffer cho TextEdit, thay vì
    // content/right_raw gốc — để buffer và phần hiển thị luôn khớp ký tự-với
    // -ký tự, giữ đúng cơ chế cursor/selection mô tả ở editor_pane.
    let left_text = left_job.text.clone();
    let right_text = right_job.text.clone();

    let spacing = 6.0;
    let total_width = ui.available_width();
    let left_width = ((total_width - spacing) * state.preview_split_ratio).max(80.0);
    let right_width = (total_width - spacing - left_width).max(80.0);
    let available_height = ui.available_height();

    ui.horizontal(|ui| {
        ui.allocate_ui(egui::vec2(left_width, available_height), |ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(left_title).strong());
                editor_pane(ui, "left_pane", &left_text, left_job, &mut state.split_scroll.offset_y);
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
                editor_pane(ui, "right_pane", &right_text, right_job, &mut state.split_scroll.offset_y);
            });
        });
    });
}

/// Pane chỉ đọc nhưng vẫn select/copy được, dùng trick chính thức của egui:
/// đưa `&mut &str` vào TextEdit thay vì `&mut String`. Luôn dùng `.layouter()`
/// để vẽ theo `job` đã tính sẵn ở diff.rs (tô màu hoặc không tuỳ chế độ) thay
/// vì để egui tự layout monospace mặc định — `content` truyền vào ĐÃ CHÍNH LÀ
/// `job.text` (xem lời gọi ở show()) nên buffer và phần hiển thị luôn khớp
/// ký tự-với-ký tự, cursor/selection định vị đúng dù việc vẽ đến từ `job`
/// chứ không phải buf mà layouter nhận vào.
///
/// KHÔNG override `job.wrap.max_width` theo `wrap_width` egui gợi ý (khác
/// bản trước) — job đã tự đặt max_width = INFINITY ở diff.rs (tắt hẳn xuống
/// dòng tự động) để 1 dòng logic luôn chiếm đúng 1 dòng hiển thị ở CẢ 2 bên,
/// không phụ thuộc bề rộng khung. Dòng dài thì tràn ngang — `ScrollArea::both()`
/// bên dưới cho cuộn ngang để xem hết, cuộn dọc vẫn đồng bộ 2 bên như cũ.
///
/// Đồng bộ scroll: 2 pane dùng chung 1 biến offset dọc trong AppState — xem
/// giải thích chi tiết ở bản gốc hàm này từ Phần 1. Chỉ set offset DỌC qua
/// `vertical_scroll_offset` (không phải `scroll_offset` với x cứng = 0.0 như
/// bản trước) — đặt cứng x=0.0 mỗi frame sẽ khiến cuộn ngang không bao giờ
/// giữ được vị trí (bị kéo về 0 ngay frame sau), cuộn ngang cần để MỖI BÊN
/// tự quản lý độc lập.
fn editor_pane(
    ui: &mut egui::Ui,
    id_salt: &str,
    content: &str,
    job: egui::text::LayoutJob,
    shared_offset: &mut f32,
) {
    let mut text = content;
    let output = egui::ScrollArea::both()
        .id_salt(id_salt)
        .auto_shrink([false, false])
        .vertical_scroll_offset(*shared_offset)
        .show(ui, |ui| {
            let mut layouter = move |ui: &egui::Ui, _buf: &dyn egui::TextBuffer, _wrap_width: f32| {
                ui.fonts_mut(|f| f.layout_job(job.clone()))
            };
            ui.add(
                egui::TextEdit::multiline(&mut text)
                    .desired_width(f32::INFINITY)
                    .layouter(&mut layouter),
            );
        });

    if (output.state.offset.y - *shared_offset).abs() > 0.5 {
        *shared_offset = output.state.offset.y;
    }
}
