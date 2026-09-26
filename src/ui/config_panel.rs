use eframe::egui;

use crate::state::{AppState, Language};

pub fn show(ui: &mut egui::Ui, state: &mut AppState) {
    let mut needs_rebuild = false;

    ui.add_space(4.0);
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

        ui.separator();

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
            // Nút to, nổi bật theo yêu cầu Phần 1; logic lưu thật (Phần 5) nối
            // qua export::save_all — lặp toàn bộ file New, ghi từng ngôn ngữ
            // đã tích ra đĩa cạnh file gốc, tự chống ghi đè bằng hậu tố _v2...
            let save_btn = egui::Button::new(
                egui::RichText::new("💾 Lưu Tất Cả File").size(16.0).strong(),
            )
            .min_size(egui::vec2(200.0, 40.0))
            .fill(egui::Color32::from_rgb(35, 120, 80));

            if ui.add(save_btn).clicked() {
                let report = crate::export::save_all(state);
                state.status_message = Some(report.summary());
            }
            if let Some(msg) = &state.status_message {
                ui.weak(msg);
            }
        });
    });
    ui.add_space(4.0);

    // Đổi SAU khi vẽ xong toàn bộ panel (không rebuild giữa chừng trong lúc
    // đang mượn state cho các closure ui.horizontal ở trên).
    if needs_rebuild {
        state.rebuild_pipeline();
    }
}
