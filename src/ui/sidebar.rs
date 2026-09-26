use eframe::egui;

use crate::state::{AppState, FileId, FileRole};

pub fn show(ui: &mut egui::Ui, state: &mut AppState) {
    ui.heading("📄 File đã nạp");
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        if ui.button("🗑 Xóa mục chọn").clicked() {
            state.remove_selected();
        }
        if ui.button("🧹 Xóa tất cả").clicked() {
            state.remove_all();
        }
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
            let mut clicked: Option<FileId> = None;

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
                if ui.selectable_label(selected == Some(file.id), label).clicked() {
                    clicked = Some(file.id);
                }
            }

            if clicked.is_some() {
                state.selected_file = clicked;
            }
        });
}
