use eframe::egui;

use crate::state::AppState;

pub struct AutoHtmlTaggerApp {
    pub state: AppState,
}

impl AutoHtmlTaggerApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let font_warning = setup_vietnamese_font(&cc.egui_ctx);

        let mut state = AppState::default();
        state.translator.set_repaint_context(cc.egui_ctx.clone());

        // Hiện ngay lúc khởi động nếu có cảnh báo — người dùng thấy được mà
        // không cần thao tác gì. Ưu tiên cảnh báo font vì ảnh hưởng hiển thị
        // toàn app; cảnh báo dict (nếu có) sẽ ghi đè nếu CẢ 2 cùng xảy ra,
        // vì thực tế hiếm khi trùng và dict quan trọng hơn để biết ngay.
        state.status_message = state
            .translator
            .dict_load_warning
            .take()
            .or(font_warning);

        Self { state }
    }
}

/// Font mặc định của egui thiếu glyph cho khối Unicode Latin Extended
/// Additional (U+1E00–U+1EFF — chứa các ký tự có dấu tổ hợp như ạ/ệ/ữ...)
/// nên chữ có dấu tiếng Việt hiện thành ô vuông/ký tự lỗi (tofu).
///
/// Nạp font hệ thống Windows có sẵn (Segoe UI, phủ tốt tiếng Việt) làm ưu
/// tiên số 1 cho cả 2 family — không cần tải/bundle file font riêng, build
/// lại là thấy hiệu quả ngay. Dùng std::fs::read (runtime) thay vì
/// include_bytes! (compile-time) đúng vì lý do đó: không phụ thuộc 1 file
/// phải có sẵn TRƯỚC khi build.
///
/// Đánh đổi: phụ thuộc đường dẫn Windows cụ thể, không portable sang máy
/// khác/OS khác. Nếu sau này cần đóng gói app để chạy trên máy KHÔNG chắc có
/// Segoe UI (vd máy Linux, hoặc dùng để phân phối cho người khác), nên
/// chuyển sang bundle font qua include_bytes! (tải "Noto Sans" tại
/// https://fonts.google.com/noto/specimen/Noto+Sans, đặt vào assets/) —
/// portable tuyệt đối vì font nằm ngay trong binary.
///
/// CHƯA xử lý tiếng Trung/Nhật ở đây — khả năng cao cũng lỗi tương tự (font
/// CJK trên Windows thường là .ttc, egui chỉ khai hỗ trợ .ttf/.otf nên chưa
/// chắc nạp thẳng được, cần kiểm tra riêng) — báo mình nếu bạn thấy chữ
/// Trung/Nhật cũng lỗi để xử lý tiếp.
fn setup_vietnamese_font(ctx: &egui::Context) -> Option<String> {
    const CANDIDATES: &[&str] = &[
        r"C:\Windows\Fonts\segoeui.ttf",
        r"C:\Windows\Fonts\arial.ttf",
    ];

    let Some(font_bytes) = CANDIDATES.iter().find_map(|path| std::fs::read(path).ok()) else {
        return Some(
            "Không tìm thấy font hệ thống (Segoe UI/Arial) để sửa hiển thị tiếng Việt — \
             xem ghi chú ở app.rs::setup_vietnamese_font để chuyển sang bundle font riêng."
                .to_string(),
        );
    };

    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "vietnamese".to_owned(),
        std::sync::Arc::new(egui::FontData::from_owned(font_bytes)),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "vietnamese".to_owned());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "vietnamese".to_owned());
    ctx.set_fonts(fonts);
    None
}

impl eframe::App for AutoHtmlTaggerApp {
    // LƯU Ý: eframe 0.36 đổi trait App::update(ctx, frame) (bản cũ) thành
    // App::ui(ui, frame) — panel giờ .show(ui, ...) thay vì .show(ctx, ...).
    // Rất nhiều tutorial/blog cũ trên mạng vẫn còn dùng update(), build sẽ lỗi
    // nếu copy nguyên mẫu code đó vào bản eframe hiện tại.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Hút kết quả dịch API chạy nền (nếu có) về TRƯỚC khi vẽ, để frame
        // này phản ánh bản dịch mới nhất thay vì trễ 1 nhịp.
        if self.state.translator.poll_background() {
            self.state.rebuild_pipeline();
        }

        // SỬA LỖI: trước đây lỗi API dịch online (hết hạn mức MyMemory, mất
        // mạng, response bất thường...) bị NUỐT MẤT HOÀN TOÀN bên trong
        // Translator — người dùng chỉ thấy im lặng không dịch được gì, không
        // có manh mối nào để tự chẩn đoán là do mạng, do hết hạn mức, hay do
        // lỗi khác. Hiện lỗi thật ra status_message ngay khi vừa phát sinh.
        // Chỉ GHI ĐÈ khi CÓ lỗi mới (take() trả None thì giữ nguyên
        // status_message hiện tại) — tránh xoá mất thông báo khác (vd kết
        // quả Lưu file) chỉ vì không có lỗi mới ở frame này.
        if let Some(err) = self.state.translator.take_last_api_error() {
            self.state.status_message = Some(format!("⚠ Dịch online lỗi: {err}"));
        }

        crate::ui::drop_zone::handle_global_drop(ui.ctx(), &mut self.state);

        // egui 0.36 hợp nhất SidePanel + TopBottomPanel thành 1 type Panel
        // (left/right/top/bottom), builder method cũng đổi tên theo hướng
        // trung lập: default_width/height -> default_size, width/height_range
        // -> size_range. Xác nhận trực tiếp qua docs.rs, không phải đoán.
        egui::Panel::top("drop_zones_panel")
            .show(ui, |ui| crate::ui::drop_zone::show(ui, &mut self.state));

        egui::Panel::bottom("config_panel")
            .show(ui, |ui| crate::ui::config_panel::show(ui, &mut self.state));

        egui::Panel::left("file_sidebar")
            .resizable(true)
            .default_size(220.0)
            .size_range(160.0..=400.0)
            .show(ui, |ui| crate::ui::sidebar::show(ui, &mut self.state));

        egui::CentralPanel::default()
            .show(ui, |ui| crate::ui::preview::show(ui, &mut self.state));
    }
}
