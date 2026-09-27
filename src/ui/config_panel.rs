use eframe::egui;

use crate::state::{AppState, Language};

pub fn show(ui: &mut egui::Ui, state: &mut AppState) {
    let mut needs_rebuild = false;

    ui.add_space(4.0);

    // 3 nhóm ĐẦU (Ngôn ngữ xuất, Tùy chọn, trạng thái/thống kê) xếp tuần tự
    // bình thường bằng ui.vertical/ui.separator — cơ chế đã ổn định xuyên
    // suốt cả app, không có gì đặc biệt. Riêng nút Bắt đầu (nhóm cuối) mới
    // cần with_layout để đẩy sát mép phải — xem comment ngay phía trên nó.
    ui.horizontal(|ui| {
        // Ngôn ngữ xuất ĐỨNG ĐẦU (góc trái cùng) theo yêu cầu mới.
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

        // Tùy chọn: 2 checkbox xếp CÙNG 1 HÀNG NGANG (trước đây xếp chồng 2
        // dòng) — nhãn rút ngắn lại, giải thích đầy đủ chuyển sang tooltip
        // hover để không chiếm chỗ nhưng vẫn tra cứu được khi cần.
        ui.vertical(|ui| {
            ui.label(egui::RichText::new("Tùy chọn").strong());
            ui.horizontal(|ui| {
                if ui
                    .checkbox(&mut state.config.use_online_translation_api, "API dịch online")
                    .on_hover_text("Dùng API dịch online cho những đoạn chưa có sẵn trong từ điển cục bộ.")
                    .changed()
                {
                    needs_rebuild = true;
                }
                if ui
                    .checkbox(&mut state.config.append_lang_suffix_to_id, "Hậu tố ngôn ngữ")
                    .on_hover_text("Thêm hậu tố ngôn ngữ (_en/_vi/_zh/_ja) vào data-builder-id khi xuất file.")
                    .changed()
                {
                    needs_rebuild = true;
                }
            });
        });

        ui.separator();

        ui.vertical(|ui| {
            if let Some(msg) = &state.status_message {
                ui.weak(msg);
            }

            // Thống kê rút gọn (bản đầy đủ trước đây quá dài, dễ chèn lên
            // cụm bên cạnh khi xếp lại layout) — vẫn đủ 4 số cốt lõi: khớp
            // từ điển, dịch qua API, chủ động bỏ qua (số đt/email/placeholder
            // giả — xem translate::should_skip_api), còn lại chưa dịch. Chỉ
            // hiện khi đã chọn ít nhất 1 ngôn ngữ output.
            if !state.config.output_languages.is_empty() {
                let s = &state.translation_stats;
                ui.weak(format!(
                    "📊 {} từ điển + {} online · {} bỏ qua · {} chưa dịch",
                    s.dict_hits, s.online_hits, s.skipped_filler, s.untranslated
                ));
            }
        });

        // Đẩy nút Bắt đầu SÁT MÉP PHẢI, cùng hàng với 3 nhóm phía trên —
        // with_layout(right_to_left) đặt Ở CUỐI ui.horizontal (sau khi đã có
        // Tùy chọn/Ngôn ngữ xuất/trạng thái đứng trước) và bên trong CHỈ
        // chứa DUY NHẤT nút, không lồng thêm ui.horizontal/vertical nào
        // khác. Khác cách làm lần trước (with_layout đứng NGAY ĐẦU hàng —
        // không có gì phía trước để egui neo bề rộng theo — rồi còn lồng
        // thêm 1 lớp ui.horizontal bên trong), đây đúng hình dạng đã thấy
        // hoạt động ổn định trong các UI kit dùng egui (nút cuối cùng của 1
        // hàng nhiều nhóm, được with_layout đẩy ra hết mép phải).
        let translating =
            state.config.use_online_translation_api && state.translator.has_pending_translations();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Nút to, nổi bật theo yêu cầu Phần 1; logic lưu thật (Phần 5)
            // nối qua export::save_all — lặp toàn bộ file New, ghi từng
            // ngôn ngữ đã tích ra đĩa cạnh file gốc, tự chống ghi đè bằng
            // hậu tố _v2...
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
