mod app;
mod diff;
mod export;
mod state;
mod tagger;
mod translate;
mod ui;

use eframe::egui;

/// Kích thước cửa sổ (đơn vị điểm logic — chưa nhân hệ số DPI) dùng cho CẢ
/// kích thước TỐI THIỂU lẫn kích thước MẶC ĐỊNH lúc mở app: app chủ đích
/// khởi động ở trạng thái nhỏ gọn nhất, người dùng cần rộng hơn thì tự kéo
/// giãn. Dùng chung 1 hằng để 2 giá trị không bao giờ lệch nhau.
const WINDOW_MIN_SIZE: [f32; 2] = [960.0, 600.0];

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(WINDOW_MIN_SIZE)
            .with_min_inner_size(WINDOW_MIN_SIZE),
        ..Default::default()
    };

    eframe::run_native(
        "Auto HTML Tagger",
        native_options,
        Box::new(|cc| Ok(Box::new(app::AutoHtmlTaggerApp::new(cc)))),
    )
}
