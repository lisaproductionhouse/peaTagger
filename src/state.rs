use eframe::egui;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    New,
    Update,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileRole {
    New,
    Old,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    En,
    Vi,
    Zh,
    Ja,
}

impl Language {
    pub const ALL: [Language; 4] = [Language::En, Language::Vi, Language::Zh, Language::Ja];

    pub fn label(self) -> &'static str {
        match self {
            Language::En => "EN",
            Language::Vi => "VI",
            Language::Zh => "ZH",
            Language::Ja => "JA",
        }
    }

    /// Hậu tố chèn vào data-builder-id khi checkbox "Thêm hậu tố ngôn ngữ vào
    /// ID" bật (Phần 5) — vd "main_paragraph_1" -> "main_paragraph_1_en".
    pub fn id_suffix(self) -> &'static str {
        match self {
            Language::En => "_en",
            Language::Vi => "_vi",
            Language::Zh => "_zh",
            Language::Ja => "_ja",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId(pub usize);

/// Parser HTML5 (html5ever) gần như không bao giờ "fail" thật sự — theo spec,
/// nó luôn cố phục hồi từ HTML lỗi thay vì báo lỗi cứng — nên trên thực tế
/// FileError hiện chỉ phát sinh từ I/O. Giữ dạng enum để mở rộng sau này mà
/// không phải đổi kiểu dữ liệu.
#[derive(Debug, Clone)]
pub enum FileError {
    ReadFailed(String),
}

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileError::ReadFailed(msg) => write!(f, "Không đọc được file: {msg}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct HtmlFile {
    pub id: FileId,
    pub path: PathBuf,
    pub role: FileRole,
    /// Nội dung gốc dạng string, đọc thẳng từ đĩa lúc kéo-thả.
    pub content: String,
    /// Nội dung file Cũ tương ứng, có được sau khi khớp theo tên file trong
    /// `AppState::rebuild_pipeline`. Chỉ có ý nghĩa với file role = New.
    pub matched_old_content: Option<String>,
    /// Kết quả sau khi chạy qua tagger engine (Phần 3) — đã gắn
    /// data-builder-id/data-editable nhưng CHƯA dịch, dùng cho pane "Đã gắn
    /// tag" và làm input cho bước dịch. None nghĩa là chưa xử lý (role =
    /// Old, hoặc file đang lỗi).
    pub pending_html: Option<String>,
    /// Bản dịch theo từng ngôn ngữ output đã chọn (Phần 4), suy ra từ
    /// pending_html. Rỗng nếu chưa có ngôn ngữ nào được chọn hoặc file chưa
    /// qua tagger.
    pub translated_by_lang: HashMap<Language, String>,
    pub error: Option<FileError>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SplitScroll {
    pub offset_y: f32,
}

pub struct ConfigState {
    pub output_languages: HashSet<Language>,
    pub use_online_translation_api: bool,
    pub append_lang_suffix_to_id: bool,
}

impl Default for ConfigState {
    fn default() -> Self {
        Self {
            output_languages: HashSet::from([Language::En, Language::Vi]),
            use_online_translation_api: false,
            // Phần 5 nói rõ tuỳ chọn hậu tố "mặc định bật" — Phần 1 mình từng
            // đặt false vì lúc đó chưa có thông tin này, sửa lại cho khớp.
            append_lang_suffix_to_id: true,
        }
    }
}

pub struct AppState {
    pub mode: AppMode,
    pub files: Vec<HtmlFile>,
    next_file_id: usize,
    pub selected_file: Option<FileId>,
    pub config: ConfigState,
    pub split_scroll: SplitScroll,
    pub preview_split_ratio: f32,
    pub status_message: Option<String>,
    /// Pane phải đang hiển thị ngôn ngữ nào — None = bản đã tag nhưng chưa
    /// dịch (pending_html). Chỉ là lựa chọn hiển thị, không ảnh hưởng dữ liệu.
    pub preview_lang: Option<Language>,
    /// Bật/tắt hiển thị diff màu (Phần 5) — mặc định tắt vì tính diff tốn
    /// thêm chi phí mỗi frame so với hiện text thường.
    pub show_diff: bool,

    pub new_zone_rect: egui::Rect,
    pub old_zone_rect: egui::Rect,

    pub translator: crate::translate::Translator,
    /// Thống kê lượt dịch GẦN NHẤT (tổng hợp qua mọi file + mọi ngôn ngữ
    /// output đã chọn) — cập nhật mỗi lần `rebuild_pipeline` chạy, hiện lên
    /// UI qua `ui::config_panel` để người dùng biết rõ tiến độ dịch: bao
    /// nhiêu khớp từ điển sẵn có, bao nhiêu vừa dịch qua API online, bao
    /// nhiêu còn chưa dịch được.
    pub translation_stats: crate::translate::TranslationStats,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            mode: AppMode::New,
            files: Vec::new(),
            next_file_id: 0,
            selected_file: None,
            config: ConfigState::default(),
            split_scroll: SplitScroll::default(),
            preview_split_ratio: 0.5,
            status_message: None,
            preview_lang: None,
            show_diff: false,
            new_zone_rect: egui::Rect::NOTHING,
            old_zone_rect: egui::Rect::NOTHING,
            translator: crate::translate::Translator::new(),
            translation_stats: crate::translate::TranslationStats::default(),
        }
    }
}

/// Bóc hậu tố phiên bản `_vN` (export::unique_path chèn khi tránh ghi đè)
/// rồi hậu tố ngôn ngữ (Language::id_suffix chèn lúc export) khỏi TÊN FILE,
/// để so khớp New<->Old theo tên gốc của template — Old thực tế luôn là 1
/// file ĐÃ EXPORT (vd "index_en.html"), không bao giờ trùng tên nguyên văn
/// với New ("index.html"). Không cần lặp nhiều lớp như bên detect.rs: mỗi
/// loại hậu tố chỉ xuất hiện tối đa 1 lần trong 1 tên file thật.
fn match_key(path: &Path) -> String {
    let mut key = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    if let Some(idx) = key.rfind("_v") {
        let digits = &key[idx + 2..];
        if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
            key.truncate(idx);
        }
    }

    for lang in Language::ALL {
        if let Some(base) = key.strip_suffix(lang.id_suffix()) {
            key = base.to_string();
            break;
        }
    }

    key
}

impl AppState {
    pub fn add_file(&mut self, path: PathBuf, role: FileRole) {
        let id = FileId(self.next_file_id);
        self.next_file_id += 1;

        let (content, error) = match std::fs::read_to_string(&path) {
            Ok(text) => (text, None),
            Err(e) => (String::new(), Some(FileError::ReadFailed(e.to_string()))),
        };

        self.files.push(HtmlFile {
            id,
            path,
            role,
            content,
            matched_old_content: None,
            pending_html: None,
            translated_by_lang: HashMap::new(),
            error,
        });
        self.selected_file = Some(id);
    }

    pub fn selected(&self) -> Option<&HtmlFile> {
        let id = self.selected_file?;
        self.files.iter().find(|f| f.id == id)
    }

    /// Xóa đúng 1 file theo id — dùng cho nút xóa nhanh (✖) hiện khi hover
    /// từng dòng trong sidebar (thay cho nút "Xóa mục chọn" cũ, vốn chỉ xóa
    /// được file ĐANG CHỌN chứ không xóa được file bất kỳ). Nếu file bị xóa
    /// đang là file đang chọn thì bỏ chọn luôn, tránh `selected_file` trỏ tới
    /// 1 id không còn tồn tại.
    pub fn remove_file(&mut self, id: FileId) {
        if self.selected_file == Some(id) {
            self.selected_file = None;
        }
        self.files.retain(|f| f.id != id);
        self.rebuild_pipeline();
    }

    pub fn remove_all(&mut self) {
        self.files.clear();
        self.selected_file = None;
    }

    /// Khớp file New<->Old theo match_key (tên gốc, đã bóc hậu tố _vN và
    /// _en/_vi/_zh/_ja) -> chạy tagger (Phần 3) -> dịch sang từng ngôn ngữ
    /// output đã chọn (Phần 4). Gọi lại sau mỗi lần danh sách file HOẶC kết
    /// quả dịch nền thay đổi.
    ///
    /// Chạy lại toàn bộ mỗi lần thay vì cập nhật tăng dần — đơn giản hơn
    /// nhiều để tránh bug; dict lookup là HashMap nên rẻ, tagger cũng chỉ
    /// parse lại text đã có sẵn trong bộ nhớ, không có I/O nào trong hàm này.
    pub fn rebuild_pipeline(&mut self) {
        // HashMap: 2 file Old khác nhau tình cờ cùng match_key thì file xử
        // lý SAU ghi đè file trước.
        let old_by_key: HashMap<String, String> = self
            .files
            .iter()
            .filter(|f| f.role == FileRole::Old)
            .map(|f| (match_key(&f.path), f.content.clone()))
            .collect();

        let target_langs: Vec<Language> = self.config.output_languages.iter().copied().collect();
        let use_api = self.config.use_online_translation_api;
        let mut total_stats = crate::translate::TranslationStats::default();

        for file in self.files.iter_mut().filter(|f| f.role == FileRole::New) {
            if file.error.is_some() {
                continue;
            }
            let matched = old_by_key.get(&match_key(&file.path)).cloned();

            let tagged = crate::tagger::tag_html(&file.content, matched.as_deref());

            file.translated_by_lang.clear();
            for &lang in &target_langs {
                let (out, stats) = self.translator.translate_html_for_lang(&tagged, lang, use_api);
                total_stats.merge(stats);
                // Chèn hậu tố NGAY ở đây (không phải lúc lưu file) để pane xem
                // trước và file thực sự ghi ra đĩa luôn khớp nhau tuyệt đối —
                // cả 2 đều đọc từ translated_by_lang, không có đường tính riêng.
                let out = if self.config.append_lang_suffix_to_id {
                    crate::tagger::append_id_suffix(&out, lang.id_suffix())
                } else {
                    out
                };
                file.translated_by_lang.insert(lang, out);
            }

            file.pending_html = Some(tagged);
            file.matched_old_content = matched;
        }

        self.translation_stats = total_stats;
        self.translator.spawn_pending_batch();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_key_strips_version_then_language_suffix() {
        assert_eq!(match_key(Path::new("index.html")), "index");
        assert_eq!(match_key(Path::new("index_en.html")), "index");
        assert_eq!(match_key(Path::new("index_en_v2.html")), "index");
        assert_eq!(match_key(Path::new("index_en_v10.html")), "index");
    }

    #[test]
    fn match_key_does_not_mistake_a_real_name_for_a_version_suffix() {
        // "_v" theo sau không phải toàn chữ số -> không bị coi là _vN.
        assert_eq!(match_key(Path::new("hero_video.html")), "hero_video");
    }
}
