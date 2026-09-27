use eframe::egui;

use crate::state::{AppState, Language};

pub fn show(ui: &mut egui::Ui, state: &mut AppState) {
    let mut needs_rebuild = false;

    ui.add_space(4.0);

    // BỎ with_layout(right_to_left) đã dùng ở bản trước — thực tế nó không chỉ
    // "dịch chuyển nhẹ" mà làm MẤT hẳn nút và đẩy cụm ngôn ngữ xuống hàng
    // riêng (chưa xác định chắc được cơ chế chính xác trong egui 0.36, khả
    // năng cao là horizontal LTR lồng trong RTL không được cấp đủ bề rộng
    // ngay từ đầu). Quay lại `ui.horizontal` + `ui.vertical` + `ui.separator`
    // tuần tự — ĐÚNG cơ chế đã dùng ổn định xuyên suốt cả app (drop_zone,
    // preview, và chính panel này ở các bản trước) — chỉ đổi THỨ TỰ cột để
    // "Ngôn ngữ xuất" nằm ngay bên trái "Bắt đầu" (đều ở 2 cột cuối, thiên
    // về phía phải panel) thay vì canh CHÍNH XÁC theo mép phải cửa sổ.
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(egui::RichText::new("Tùy chọn").strong());
            if ui
                .checkbox(&mut state.config.use_online_translation_api, "Dùng API dịch online")
                .changed()
            {
                needs_rebuild = true;
            }
            if ui
                .checkbox(&mut state.config.append_lang_suffix_to_id, "Thêm hậu tố ngôn ngữ vào ID")
                .changed()
            {
                needs_rebuild = true;
            }
        });

        ui.separator();

        ui.vertical(|ui| {
            ui.label(egui::RichText::new("Ngôn ngữ xuất").strong());
            ui.horizontal(|ui| {
                for lang in Language::ALL {
                    let mut checked = state.config.output_languages.contains(&lang);
                    if ui.checkbox(&mut checked, lang.label()).changed() {
                        if checked {
                            state.config.output_languages.insert(lang);
                        } else {
                            state.config.output_languages.remove(&lang);
                        }
                        needs_rebuild = true;
                    }
                }
            });
        });

        ui.separator();

        ui.vertical(|ui| {
            // Nút to, nổi bật theo yêu cầu Phần 1; logic lưu thật (Phần 5) nối
            // qua export::save_all — lặp toàn bộ file New, ghi từng ngôn ngữ
            // đã tích ra đĩa cạnh file gốc, tự chống ghi đè bằng hậu tố _v2...
            let start_btn = egui::Button::new(egui::RichText::new("▶ Bắt đầu").size(16.0).strong())
                .min_size(egui::vec2(160.0, 40.0))
                .fill(egui::Color32::from_rgb(35, 120, 80));

            // SỬA LỖI: làm mờ (disable) nút trong lúc còn bản dịch đang chờ
            // API dịch online trả kết quả — trước đây nút luôn bấm được
            // ngay, dùng bất cứ gì đang có trong translated_by_lang tại
            // đúng thời điểm bấm mà KHÔNG đợi thread nền dịch xong, có thể
            // xuất ra file lẫn lộn phần đã dịch/còn nguyên tiếng Anh trong
            // im lặng. export::save_all vẫn giữ nguyên phần chặn tương ứng
            // làm lớp an toàn cuối.
            let translating = state.config.use_online_translation_api
                && state.translator.has_pending_translations();
            let start_resp = ui.add_enabled(!translating, start_btn).on_hover_text(if translating {
                "Đang chờ API dịch online trả kết quả — đợi vài giây rồi thử lại."
            } else {
                "Lưu tất cả file đã nạp ra đĩa, theo các ngôn ngữ output đã chọn ở trên."
            });

            if start_resp.clicked() {
                let report = crate::export::save_all(state);
                state.status_message = Some(report.summary());
            }
            if let Some(msg) = &state.status_message {
                ui.weak(msg);
            }

            // Thống kê: bao nhiêu đoạn khớp sẵn trong từ điển (local_dict.json),
            // bao nhiêu vừa dịch qua API online TRONG phiên này, bao nhiêu bị
            // CHỦ ĐỘNG bỏ qua không gửi API (số đt/email/placeholder giả — xem
            // translate::should_skip_api), còn lại bao nhiêu chưa dịch được.
            // Chỉ hiện khi đã chọn ít nhất 1 ngôn ngữ output.
            if !state.config.output_languages.is_empty() {
                let s = &state.translation_stats;
                ui.weak(format!(
                    "📊 Đã dịch: {} (từ điển) + {} (online) — {} bỏ qua (số đt/email/placeholder) — còn {} chưa dịch",
                    s.dict_hits, s.online_hits, s.skipped_filler, s.untranslated
                ));
            }
        });
    });

    ui.add_space(4.0);

    // Đổi SAU khi vẽ xong toàn bộ panel (không rebuild giữa chừng trong lúc
    // đang mượn state cho các closure ở trên).
    if needs_rebuild {
        state.rebuild_pipeline();
    }
}
