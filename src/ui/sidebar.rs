use eframe::egui;

use crate::state::{AppState, FileId, FileRole};

pub fn show(ui: &mut egui::Ui, state: &mut AppState) {
    // Tiêu đề + "Xóa tất cả" cùng 1 hàng. Nút được with_layout(right_to_left)
    // đẩy sát mép phải, đặt Ở CUỐI hàng và bên trong chỉ có đúng 1 widget —
    // đúng hình dạng đã xác nhận chạy ổn ở nút "Bắt đầu" (config_panel.rs).
    ui.horizontal(|ui| {
        ui.heading("📄 File đã nạp");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("🧹 Xóa tất cả").clicked() {
                state.remove_all();
            }
        });
    });
    ui.separator();

    egui::ScrollArea::vertical()
        .id_salt("sidebar_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if state.files.is_empty() {
                ui.weak("Chưa có file nào. Kéo thả ở phía trên để bắt đầu.");
                return;
            }

            let selected = state.selected_file;
            let row_height = ui.spacing().interact_size.y + 6.0;
            let font_id = egui::TextStyle::Body.resolve(ui.style());
            let mut clicked: Option<FileId> = None;
            let mut to_remove: Option<FileId> = None;

            for file in &state.files {
                let name = file
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "?".to_string());
                // Badge role giúp phân biệt New/Old khi ở chế độ Update — spec gốc
                // không nói rõ 2 nhóm file hiển thị tách nhau hay gộp chung, mình
                // chọn gộp chung 1 danh sách + badge để đơn giản cho khung UI này.
                let badge = match file.role {
                    FileRole::New => "New",
                    FileRole::Old => "Old",
                };
                let prefix = if file.error.is_some() { "⚠ " } else { "" };
                let label = format!("{prefix}[{badge}] {name}");

                // MỖI DÒNG = 1 VÙNG BẤM DUY NHẤT phủ kín bề ngang sidebar (không
                // ghép label + nút thành 2 widget cạnh nhau): nhờ vậy hover được
                // tính trên CẢ dòng, chuột di từ chữ sang nút ✖ không bao giờ
                // "rơi" qua khe hở làm ✖ biến mất, và bề rộng dòng không đổi theo
                // hover — tránh lặp lại lỗi layout dịch chuyển đã gặp ở
                // config_panel.rs. Toàn bộ nội dung dòng (nền, chữ, ✖) vẽ tay
                // bằng painter — không dùng egui::SelectableLabel vì widget này
                // đã bị gỡ khỏi các bản egui mới.
                let (row_rect, row_resp) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), row_height),
                    egui::Sense::click(),
                );
                // Ô ✖ vuông, sát lề phải dòng.
                let icon_rect = egui::Rect::from_min_size(
                    egui::pos2(row_rect.right() - row_height, row_rect.top()),
                    egui::vec2(row_height, row_height),
                );
                let hovered = row_resp.hovered();
                let over_icon = hovered
                    && ui
                        .input(|i| i.pointer.interact_pos())
                        .is_some_and(|p| icon_rect.contains(p));
                let is_selected = selected == Some(file.id);

                let painter = ui.painter();
                if is_selected {
                    painter.rect_filled(
                        row_rect,
                        egui::CornerRadius::same(3),
                        ui.visuals().selection.bg_fill,
                    );
                } else if hovered {
                    painter.rect_filled(
                        row_rect,
                        egui::CornerRadius::same(3),
                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 14),
                    );
                }

                // Chữ cắt (clip) ngay trước ô ✖ — tên dài không bao giờ chạy
                // đè lên nút xóa.
                let text_area = egui::Rect::from_min_max(
                    egui::pos2(row_rect.left() + 6.0, row_rect.top()),
                    egui::pos2(icon_rect.left(), row_rect.bottom()),
                );
                let text_color = if is_selected {
                    ui.visuals().selection.stroke.color
                } else {
                    ui.visuals().text_color()
                };
                let text_rect = ui.painter_at(text_area).text(
                    text_area.left_center(),
                    egui::Align2::LEFT_CENTER,
                    &label,
                    font_id.clone(),
                    text_color,
                );

                // ✖ chỉ hiện khi hover dòng; sáng đỏ + nền mờ khi chuột nằm
                // đúng trên ô ✖. Không hover thì không vẽ gì — vùng này vẫn
                // được giữ chỗ (icon_rect cố định) nên dòng không đổi kích thước.
                if hovered {
                    if over_icon {
                        painter.rect_filled(
                            icon_rect.shrink(2.0),
                            egui::CornerRadius::same(3),
                            egui::Color32::from_rgba_unmultiplied(255, 90, 90, 45),
                        );
                    }
                    painter.text(
                        icon_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "✖",
                        egui::FontId::proportional(13.0),
                        if over_icon {
                            egui::Color32::from_rgb(255, 120, 120)
                        } else {
                            egui::Color32::from_gray(160)
                        },
                    );
                }

                // Bấm đúng ô ✖ -> xóa ngay (không hỏi xác nhận); bấm chỗ khác
                // trên dòng -> chọn file như trước.
                if row_resp.clicked() {
                    if over_icon {
                        to_remove = Some(file.id);
                    } else {
                        clicked = Some(file.id);
                    }
                }

                // Tên bị cắt -> hover hiện tooltip tên đầy đủ.
                if text_rect.width() > text_area.width() {
                    let _ = row_resp.on_hover_text(label);
                }
            }

            // Áp thay đổi SAU vòng lặp (đang mượn &state.files ở trên).
            if let Some(id) = to_remove {
                state.remove_file(id);
            }
            if let Some(id) = clicked {
                state.selected_file = Some(id);
            }
        });
}
