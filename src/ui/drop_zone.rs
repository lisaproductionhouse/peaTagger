use eframe::egui;
use std::path::PathBuf;

use crate::state::{AppMode, AppState, FileRole};

/// Đọc các file mà OS vừa thả vào cửa sổ trong frame này, và định tuyến
/// New/Old dựa trên vị trí con trỏ so với rect của từng zone (egui chỉ cho
/// biết CÓ file được thả, không gắn sẵn toạ độ vào từng DroppedFile, nên phải
/// tự hit-test bằng pointer position tại thời điểm thả).
///
/// LƯU Ý API: egui 0.36 đổi `DroppedFile` từ struct có field `path` thành
/// một trait với method `path()` / `bytes()` (RawInput::dropped_files giờ là
/// `Vec<DroppedFileHandle>`). Code kiểu cũ `f.path.clone()` sẽ không compile.
pub fn handle_global_drop(ctx: &egui::Context, state: &mut AppState) {
    let dropped_paths: Vec<PathBuf> = ctx.input(|i| {
        i.raw
            .dropped_files
            .iter()
            .map(|f| f.path().to_path_buf())
            .collect()
    });
    if dropped_paths.is_empty() {
        return;
    }

    let drop_pos = ctx.input(|i| i.pointer.interact_pos());
    let role = match drop_pos {
        Some(pos) if state.mode == AppMode::Update && state.old_zone_rect.contains(pos) => {
            FileRole::Old
        }
        _ => FileRole::New,
    };

    for path in dropped_paths {
        register_path(state, path, role);
    }
    // Gọi 1 LẦN sau khi thêm xong cả loạt (không gọi trong add_file) để tránh
    // chạy lại toàn bộ pipeline N lần khi thả 1 thư mục có N file.
    state.rebuild_pipeline();
}

fn register_path(state: &mut AppState, path: PathBuf, role: FileRole) {
    if path.is_dir() {
        let html_files = walkdir::WalkDir::new(&path)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("html") || ext.eq_ignore_ascii_case("htm"))
            });
        for entry in html_files {
            state.add_file(entry.into_path(), role);
        }
    } else {
        state.add_file(path, role);
    }
}

pub fn show(ui: &mut egui::Ui, state: &mut AppState) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Chế độ:").strong());
        ui.selectable_value(&mut state.mode, AppMode::New, "🆕 Tạo mới");
        ui.selectable_value(&mut state.mode, AppMode::Update, "🔄 Cập nhật");
    });
    ui.add_space(4.0);

    // Spec chỉ nói "ô File Cũ chỉ hiện khi ở chế độ Update" nhưng không nói rõ
    // người dùng chọn chế độ ở đâu — mình thêm hàng radio phía trên để đó chính
    // là điều khiển quyết định việc hiện/ẩn ô thứ 2.
    let show_old_zone = state.mode == AppMode::Update;
    ui.columns(if show_old_zone { 2 } else { 1 }, |cols| {
        state.new_zone_rect = zone(&mut cols[0], "📥 File Mới");
        if show_old_zone {
            state.old_zone_rect = zone(&mut cols[1], "🗂 File Cũ");
        }
    });
    ui.add_space(6.0);
}

/// Vẽ 1 ô kéo-thả: tiêu đề ngắn + dấu "+" lớn ở giữa (thay cho câu hướng dẫn
/// dài dòng trước đây — "+" là quy ước phổ biến hơn cho "thả nội dung vào
/// đây"), viền NÉT ĐỨT bao quanh để nhấn mạnh đây là vùng thả file.
///
/// egui::Frame chỉ hỗ trợ viền LIỀN NÉT qua `.stroke()` (không có tuỳ chọn
/// nét đứt) nên khung nét đứt được vẽ THỦ CÔNG bằng `Shape::dashed_line`
/// trên 1 đường khép kín nối 4 góc của `response.rect`, vẽ ĐÈ lên sau khi
/// Frame đã submit xong nội dung bên trong — không chồng lấn gì vì viền nằm
/// sát mép còn nội dung có inner_margin nằm lùi vào giữa.
fn zone(ui: &mut egui::Ui, title: &str) -> egui::Rect {
    let is_hovering_file = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
    let accent_color = if is_hovering_file {
        egui::Color32::from_rgb(90, 160, 255)
    } else {
        egui::Color32::GRAY
    };

    let response = egui::Frame::default()
        .inner_margin(12.0)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.set_min_height(80.0);
            ui.vertical_centered(|ui| {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(title).strong().size(13.0));
                ui.add_space(2.0);
                ui.label(egui::RichText::new("+").size(26.0).color(accent_color));
                ui.add_space(6.0);
            });
        })
        .response;

    let rect = response.rect;
    let corners = vec![
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ];
    ui.painter().extend(egui::Shape::dashed_line(
        &corners,
        egui::Stroke::new(2.0, accent_color),
        6.0,
        4.0,
    ));

    rect
}
