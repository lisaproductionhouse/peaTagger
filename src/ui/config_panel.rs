use eframe::egui;

use crate::state::{AppState, Language};

pub fn show(ui: &mut egui::Ui, state: &mut AppState) {
    let mut needs_rebuild = false;

    ui.add_space(4.0);

    // Hàng 1: Tùy chọn + trạng thái/thống kê — thông tin phụ trợ, tách khỏi
    // cụm hành động chính (Ngôn ngữ xuất | Bắt đầu) ở hàng 2 bên dưới.
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

        if let Some(msg) = &state.status_message {
            ui.separator();
            ui.weak(msg);
        }

        // TÍNH NĂNG: thống kê rõ ràng — bao nhiêu đoạn khớp sẵn trong từ điển
        // (local_dict.json), bao nhiêu vừa dịch được qua API online TRONG
        // phiên này, bao nhiêu bị CHỦ ĐỘNG bỏ qua không gửi API (số đt/email/
        // placeholder La-tinh giả — xem translate::should_skip_api), còn lại
        // bao nhiêu chưa dịch được. Chỉ hiện khi đã chọn ít nhất 1 ngôn ngữ
        // output (tránh hiện "0/0/0" vô nghĩa lúc chưa cấu hình).
        if !state.config.output_languages.is_empty() {
            ui.separator();
            let s = &state.translation_stats;
            ui.weak(format!(
                "📊 Đã dịch: {} (từ điển) + {} (online) — {} bỏ qua (số đt/email/placeholder) — còn {} chưa dịch",
                s.dict_hits, s.online_hits, s.skipped_filler, s.untranslated
            ));
        }
    });

    ui.add_space(10.0);

    // Hàng 2: cụm "Ngôn ngữ xuất | Bắt đầu" đẩy sát góc DƯỚI-PHẢI — đúng
    // chuẩn UI/UX: hành động chính (CTA) cùng điều kiện tiên quyết trực tiếp
    // của nó (chọn ngôn ngữ output) nằm ở góc phải, tách khỏi khối Tùy chọn.
    // QUAN TRỌNG: gộp cụm ngôn ngữ + nút thành 1 khối `ui.horizontal` DUY
    // NHẤT rồi mới đẩy CẢ KHỐI sang phải — with_layout(right_to_left) ở đây
    // chỉ nhận ĐÚNG 1 item (chính khối này). Lúc đầu mình để nút và cụm
    // ngôn ngữ là 2 item TÁCH RỜI ngay trong right_to_left — vị trí của cụm
    // ngôn ngữ khi đó được tính DỰA TRÊN kích thước nút vừa đo được ở CHÍNH
    // frame đó, nên 2 item bị phụ thuộc dây chuyền vào nhau thay vì đứng
    // độc lập. Gộp lại còn 1 item duy nhất loại bỏ hẳn kiểu phụ thuộc đó —
    // bên trong khối vẫn là `ui.horizontal` bình thường (trái->phải), không
    // còn chỗ nào phải "ép" vị trí theo kích thước đo được của phần tử khác.
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.horizontal(|ui| {
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

            ui.add_space(16.0);

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
        });
    });

    ui.add_space(4.0);

    // Đổi SAU khi vẽ xong toàn bộ panel (không rebuild giữa chừng trong lúc
    // đang mượn state cho các closure ở trên).
    if needs_rebuild {
        state.rebuild_pipeline();
    }
}
