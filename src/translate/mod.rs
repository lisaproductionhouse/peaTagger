mod apply;
mod backend;
mod dict;

use std::collections::{HashMap, HashSet};
use std::sync::mpsc;
use std::sync::LazyLock;

use eframe::egui;
use regex::Regex;

use crate::state::Language;
use backend::TranslationBackend;
use dict::LocalDict;

/// Ngôn ngữ được coi là NGUỒN — nội dung trong file HTML gốc được giả định
/// viết bằng ngôn ngữ này, và đây cũng là ngôn ngữ dùng làm KHOÁ trong
/// local_dict.json (xem dict.rs). Gap tự bù ban đầu (Phần 4) đặt "vi", nhưng
/// file local_dict.json mẫu người dùng cung cấp dùng khoá tiếng Anh
/// ("home"/"contact", không có "en" trong value) — tín hiệu rõ ràng nguồn
/// nên là "en". Đổi hằng số này nếu thực tế nguồn khác tiếng Anh.
const SOURCE_LANG: &str = "en";

// Ghi chú "chỉ persist đoạn NGẮN xuống local_dict.json" giờ nằm ở
// should_persist_to_dict (đặt cạnh poll_background, nơi nó được dùng) —
// tiêu chí đã đổi từ đếm KÝ TỰ sang đếm SỐ TỪ (1-12) để khớp đúng
// auto_tagger.py::_should_persist_to_dict, xem giải thích đầy đủ ở đó.

fn lang_code(lang: Language) -> &'static str {
    match lang {
        Language::En => "en",
        Language::Vi => "vi",
        Language::Zh => "zh-CN",
        Language::Ja => "ja",
    }
}

// Mẫu tương tự PHONE_RE/EMAIL_RE của detect.rs (không import trực tiếp để
// tránh phụ thuộc chéo giữa 2 module `tagger`/`translate` vốn độc lập nhau
// — mỗi module tự giữ mẫu regex NHỎ, ĐƠN GIẢN của riêng nó cho đúng nhu cầu
// cục bộ, chấp nhận trùng lặp 1 chút thay vì ràng buộc chéo không cần thiết).
static PHONE_LIKE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[+]?[\d\s\-().]{7,20}$").expect("regex tĩnh hợp lệ"));
static EMAIL_LIKE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}$").expect("regex tĩnh hợp lệ")
});

/// Từ vựng La-tinh giả thường gặp trong text placeholder — không chỉ đúng
/// đoạn "Lorem ipsum dolor sit amet..." kinh điển, mà cả các biến thể mở
/// rộng nhiều theme/generator hay dùng (vd "Nam quis accumsan risus. Aenean
/// id volutpat nibh..."). Dùng để NHẬN DIỆN và bỏ qua, không gửi lên API
/// dịch online: (a) dịch ra cũng vô nghĩa vì đây không phải tiếng Anh thật,
/// (b) trang demo/theme thường có RẤT NHIỀU đoạn dài kiểu này, tốn phần lớn
/// hạn mức API ít ỏi (~5000 ký tự/ngày, bản miễn phí) một cách vô ích, khiến
/// nội dung THẬT SỰ cần dịch bị "đói" hạn mức.
///
/// LƯU Ý: đây là HEURISTIC (đối chiếu từ vựng), không phải nhận diện ngôn
/// ngữ chính xác tuyệt đối — không bao quát HẾT mọi bộ sinh Latin giả có thể
/// tồn tại, nhưng bắt được phần lớn các biến thể phổ biến. Nếu gặp theme
/// dùng bộ từ vựng khác hẳn không có trong danh sách này, có thể bổ sung
/// thêm từ vào đây.
const LOREM_IPSUM_MARKERS: &[&str] = &[
    // Đoạn "lorem ipsum" kinh điển.
    "lorem", "ipsum", "dolor", "amet", "consectetur", "adipiscing", "elit", "eiusmod", "tempor",
    "incididunt", "labore", "dolore", "magna", "aliqua", "veniam", "nostrud", "exercitation",
    "ullamco", "laboris", "nisi", "aliquip", "commodo", "consequat", "duis", "aute", "irure",
    "reprehenderit", "voluptate", "velit", "cillum", "fugiat", "pariatur", "excepteur",
    "occaecat", "cupidatat", "proident", "culpa", "officia", "deserunt", "mollit", "laborum",
    // Biến thể mở rộng hay gặp (Aenean, Nam, Vivamus... và các từ đi kèm).
    "aenean", "nam", "vivamus", "nullam", "curabitur", "donec", "cras", "etiam", "phasellus",
    "suspendisse", "vestibulum", "maecenas", "fusce", "pellentesque", "praesent", "quisque",
    "accumsan", "volutpat", "nibh", "risus", "morbi", "malesuada", "condimentum", "scelerisque",
    "porttitor", "sagittis", "ultricies", "eleifend", "gravida", "posuere", "sollicitudin",
    "tincidunt", "vulputate", "auctor", "hendrerit", "sodales", "molestie", "lacinia", "congue",
    "rhoncus", "tortor", "faucibus", "pulvinar", "convallis", "dignissim", "lobortis",
];

/// true nếu `text` trông giống 1 đoạn placeholder La-tinh giả (đếm số TỪ
/// trùng với `LOREM_IPSUM_MARKERS`). Ngưỡng ≥2 từ khớp (không phải 1) để
/// giảm rủi ro chặn nhầm nội dung tiếng Anh thật chỉ vì trùng ngẫu nhiên 1 từ
/// (vd "Duis" cũng có thể là tên riêng có thật).
fn looks_like_lorem_ipsum(text: &str) -> bool {
    let matches = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .filter(|w| LOREM_IPSUM_MARKERS.contains(&w.to_lowercase().as_str()))
        .count();
    matches >= 2
}

/// true nếu KHÔNG nên gửi `trimmed` lên API dịch online — số điện thoại,
/// email, hoặc placeholder La-tinh giả đều (a) dịch ra vô nghĩa hoặc không
/// cần dịch, (b) API gần như chắc chắn trả về y hệt bản gốc (tốn 1 lượt gọi
/// + hạn mức mà không được gì) hoặc thất bại thẳng. `dict`/`session_cache`
/// VẪN được tra bình thường trước đó (rẻ, không tốn mạng) — hàm này CHỈ ảnh
/// hưởng bước "có gửi lên API hay không" khi tra dict/cache bị miss.
fn should_skip_api(trimmed: &str) -> bool {
    PHONE_LIKE.is_match(trimmed) || EMAIL_LIKE.is_match(trimmed) || looks_like_lorem_ipsum(trimmed)
}

/// Chuẩn hoá 1 đoạn text/attribute-value THÔ (có thể có khoảng trắng đầu/
/// cuối) để tra dict — SỬA LỖI: trước đây tra thẳng nguyên văn (phân biệt
/// hoa/thường VÀ không strip khoảng trắng), trong khi local_dict.json luôn
/// dùng key CHỮ THƯỜNG (khớp `auto_tagger.py::translate_html_content` — biến
/// `lower_text`/`lower_val` đều qua `.strip().lower()` trước khi tra) — nên
/// "SEO Help" không bao giờ khớp được key "seo help" trong dict dù dict đầy
/// đủ tới đâu, và " Home " (có khoảng trắng bọc ngoài lúc parse HTML) cũng
/// không khớp "home". Trả None nếu rỗng sau khi strip (không có gì để dịch).
fn normalize_for_lookup(raw: &str) -> Option<(&str, String)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some((trimmed, trimmed.to_lowercase()))
}

/// Ráp bản dịch tra được (`hit`, ứng với `trimmed` sau khi đã strip+lower)
/// trở lại thành kết quả cuối: khôi phục hoa/thường theo `trimmed` (bản GỐC,
/// chưa lowercase) qua `preserve_case`, rồi đặt lại đúng khoảng trắng đầu/
/// cuối của `raw` — khớp `token.replace(stripped_text, final_text)` /
/// `val.replace(stripped_val, final_val)` của auto_tagger.py (thay đúng
/// phần đã strip, giữ nguyên whitespace bao quanh), nhưng dùng cắt chuỗi
/// theo vị trí biết trước thay vì replace-theo-nội-dung để tránh mọi rủi ro
/// khớp nhầm (dù trên thực tế không thể xảy ra do cách `trimmed` được suy ra
/// từ chính `raw`).
fn apply_hit(raw: &str, trimmed: &str, hit: &str) -> String {
    let cased = preserve_case(trimmed, hit);
    let start = raw.len() - raw.trim_start().len();
    let end = raw.trim_end().len();
    format!("{}{}{}", &raw[..start], cased, &raw[end..])
}

/// Khôi phục lại hoa/thường cho bản dịch tra được từ dict, dựa theo hoa/
/// thường của văn bản GỐC (đã strip, CHƯA lowercase) — khớp CHÍNH XÁC
/// `auto_tagger.py::preserve_case` (đã đối chiếu trực tiếp với hành vi thật
/// của Python `str.isupper()/islower()/istitle()/title()/capitalize()` trên
/// nhiều ca kể cả tiếng Việt có dấu, trước khi viết hàm này).
///
/// Cần thiết vì dict được tra CASE-INSENSITIVE (xem `normalize_for_lookup`)
/// — nếu không khôi phục, "SEO Help" sẽ dịch ra y hệt dạng đã lưu trong dict
/// ("hỗ trợ seo", thường tự học qua API nên hay ở dạng thường) thay vì đúng
/// hoa/thường phù hợp ngữ cảnh hiển thị.
///
/// Thứ tự kiểm tra (ưu tiên trên xuống, y hệt bản gốc):
/// 1. Toàn bộ HOA -> bản dịch cũng viết HOA toàn bộ.
/// 2. Toàn bộ thường -> bản dịch cũng viết thường toàn bộ.
/// 3. Title Case (Mỗi Từ Viết Hoa Chữ Đầu, chữ còn lại trong từ đó thường)
///    -> bản dịch cũng chuyển sang Title Case.
/// 4. Chỉ ký tự đầu là chữ hoa (kể cả khi KHÔNG đạt chuẩn Title Case ở trên
///    — vd "SEO Help": "SEO" có nhiều hơn 1 chữ hoa liên tiếp trong cùng 1
///    "từ" nên không phải Title Case hợp lệ) -> chỉ viết hoa ký tự đầu bản
///    dịch, phần còn lại hạ về chữ thường.
/// 5. Không khớp mẫu nào (vd bắt đầu bằng số/ký hiệu) -> giữ nguyên bản dịch
///    như đã tra được.
fn preserve_case(original: &str, translated: &str) -> String {
    if original.is_empty() || translated.is_empty() {
        return translated.to_string();
    }
    if is_all_upper(original) {
        translated.to_uppercase()
    } else if is_all_lower(original) {
        translated.to_lowercase()
    } else if is_title_case(original) {
        to_title_case(translated)
    } else if original.chars().next().is_some_and(char::is_uppercase) {
        capitalize_first(translated)
    } else {
        translated.to_string()
    }
}

/// Khớp `str.isupper()` của Python: có ít nhất 1 ký tự "có hoa/thường" (chữ
/// cái), và TẤT CẢ ký tự có hoa/thường đó đều là chữ HOA (ký tự không có
/// khái niệm hoa/thường như số, dấu câu... không tính, không làm rớt điều
/// kiện).
fn is_all_upper(s: &str) -> bool {
    let mut has_cased = false;
    for c in s.chars() {
        if c.is_lowercase() {
            return false;
        }
        if c.is_uppercase() {
            has_cased = true;
        }
    }
    has_cased
}

/// Khớp `str.islower()` của Python.
fn is_all_lower(s: &str) -> bool {
    let mut has_cased = false;
    for c in s.chars() {
        if c.is_uppercase() {
            return false;
        }
        if c.is_lowercase() {
            has_cased = true;
        }
    }
    has_cased
}

/// Khớp `str.istitle()` của Python: ký tự HOA chỉ được đứng ngay sau 1 ký tự
/// KHÔNG có hoa/thường (đầu 1 "từ"), ký tự thường chỉ được đứng ngay sau 1
/// ký tự CÓ hoa/thường (giữa/cuối từ) — vd "SEO Help" KHÔNG đạt chuẩn này
/// (E, O hoa đứng ngay sau S cũng hoa, vi phạm "chỉ ký tự đầu từ được hoa"),
/// "Seo Help" mới đạt.
fn is_title_case(s: &str) -> bool {
    let mut has_cased = false;
    let mut prev_cased = false;
    for c in s.chars() {
        if c.is_uppercase() {
            if prev_cased {
                return false;
            }
            has_cased = true;
            prev_cased = true;
        } else if c.is_lowercase() {
            if !prev_cased {
                return false;
            }
            has_cased = true;
            prev_cased = true;
        } else {
            prev_cased = false;
        }
    }
    has_cased
}

/// Khớp `str.title()` của Python: viết hoa ký tự có hoa/thường ĐẦU TIÊN của
/// mỗi "từ" (chuỗi ký tự có hoa/thường liên tục, ngăn cách bởi ký tự không
/// có hoa/thường như khoảng trắng/dấu câu/số), hạ thường mọi ký tự có hoa/
/// thường còn lại trong cùng từ đó.
fn to_title_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut prev_cased = false;
    for c in s.chars() {
        let is_cased = c.is_uppercase() || c.is_lowercase();
        if is_cased {
            if prev_cased {
                result.extend(c.to_lowercase());
            } else {
                result.extend(c.to_uppercase());
            }
            prev_cased = true;
        } else {
            result.push(c);
            prev_cased = false;
        }
    }
    result
}

/// Khớp `str.capitalize()` của Python: viết hoa ký tự ĐẦU chuỗi, hạ thường
/// TOÀN BỘ phần còn lại (không phân biệt ranh giới từ như to_title_case).
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => {
            let mut result: String = first.to_uppercase().collect();
            result.extend(chars.flat_map(char::to_lowercase));
            result
        }
        None => String::new(),
    }
}

/// Thống kê 1 lượt dịch (`translate_html_for_lang`): bao nhiêu đoạn khớp sẵn
/// trong `local_dict.json` (từ điển offline — có thể đã học từ những lần
/// chạy TRƯỚC, kể cả từ chính Python), bao nhiêu đoạn khớp qua
/// `session_cache` (nghĩa là vừa dịch THÀNH CÔNG qua API online TRONG CHÍNH
/// phiên làm việc hiện tại), bao nhiêu đoạn CHỦ ĐỘNG bỏ qua không gửi API
/// (số điện thoại/email/placeholder La-tinh giả — xem `should_skip_api`),
/// và bao nhiêu đoạn còn lại THẬT SỰ chưa dịch được (đang chờ API trả lời,
/// đã thử nhưng thất bại, hoặc chưa bật "Dùng API dịch online").
///
/// `rebuild_pipeline` gom (cộng dồn) giá trị này qua MỌI file + MỌI ngôn ngữ
/// output trong 1 lượt chạy, để hiện lên UI 1 con số tổng quan duy nhất —
/// xem `AppState::translation_stats` (state.rs) và cách hiển thị ở
/// `ui::config_panel`.
#[derive(Debug, Clone, Copy, Default)]
pub struct TranslationStats {
    pub dict_hits: usize,
    pub online_hits: usize,
    pub skipped_filler: usize,
    pub untranslated: usize,
}

impl TranslationStats {
    pub fn merge(&mut self, other: TranslationStats) {
        self.dict_hits += other.dict_hits;
        self.online_hits += other.online_hits;
        self.skipped_filler += other.skipped_filler;
        self.untranslated += other.untranslated;
    }
}

/// Nguồn của 1 lượt tra trúng — phân biệt để tính `TranslationStats`: Dict =
/// có sẵn trong `local_dict.json`, Session = vừa dịch THÀNH CÔNG qua API
/// online TRONG phiên làm việc hiện tại (`session_cache` chỉ được điền
/// trong `poll_background`, không có nguồn nào khác — nên tra trúng ở đây
/// LUÔN đồng nghĩa "online mới").
enum LookupHit {
    Dict(String),
    Session(String),
}

/// SỬA LỖI: `result` giờ là `Result<String, String>` (giữ nguyên LÝ DO LỖI
/// THẬT khi thất bại) thay vì `Option<String>` (chỉ biết "có/không", mất
/// hết chi tiết) — để `poll_background` có thể lưu lại lỗi gần nhất và
/// app.rs hiện ra UI, giúp người dùng biết CHÍNH XÁC vì sao dịch online
/// không chạy (hết hạn mức MyMemory, mất mạng, response bất thường...) thay
/// vì chỉ thấy im lặng không dịch được gì mà không có manh mối nào.
struct TranslatedItem {
    text: String,
    lang: String,
    result: Result<String, String>,
}

pub struct Translator {
    dict: LocalDict,
    /// Thông báo nếu local_dict.json tồn tại nhưng lỗi cú pháp JSON lúc nạp
    /// — app.rs đọc field này 1 lần lúc khởi tạo để hiện lên status_message.
    pub dict_load_warning: Option<String>,
    /// Lỗi API dịch online GẦN NHẤT (nếu có) — app.rs đọc qua
    /// `take_last_api_error()` mỗi frame để hiện lên status_message, ngay
    /// khi vừa phát sinh (không đợi tới lúc bấm Lưu mới biết).
    last_api_error: Option<String>,
    /// Cache trong phiên làm việc cho MỌI bản dịch học được từ API, kể cả
    /// đoạn dài không persist — tránh gọi lại API cho cùng 1 đoạn dài giữa
    /// các lần rebuild_pipeline trong cùng 1 lần chạy app.
    session_cache: HashMap<(String, String), String>,
    in_flight: HashSet<(String, String)>,
    misses_this_pass: Vec<(String, String)>,
    tx: mpsc::Sender<TranslatedItem>,
    rx: mpsc::Receiver<TranslatedItem>,
    repaint_ctx: Option<egui::Context>,
}

impl Translator {
    pub fn new() -> Self {
        let (dict, dict_load_warning) = LocalDict::load();
        let (tx, rx) = mpsc::channel();
        Self {
            dict,
            dict_load_warning,
            last_api_error: None,
            session_cache: HashMap::new(),
            in_flight: HashSet::new(),
            misses_this_pass: Vec::new(),
            tx,
            rx,
            repaint_ctx: None,
        }
    }

    /// Gọi 1 lần lúc khởi tạo app (từ CreationContext) để thread nền có thể
    /// tự đánh thức UI khi dịch xong, thay vì phải đợi người dùng rê chuột.
    pub fn set_repaint_context(&mut self, ctx: egui::Context) {
        self.repaint_ctx = Some(ctx);
    }

    /// LƯU Ý: `text` truyền vào đây PHẢI đã được chuẩn hoá (strip + lower)
    /// từ trước bởi caller (xem `normalize_for_lookup`) — hàm này chỉ thuần
    /// tra cứu, không tự chuẩn hoá hộ, để khớp đúng quy ước "mọi key trong
    /// dict/cache đều là chữ thường đã strip" xuyên suốt cả session_cache
    /// LẪN local_dict.json trên đĩa.
    fn lookup(&self, text: &str, lang: &str) -> Option<LookupHit> {
        let key = (text.to_string(), lang.to_string());
        if let Some(v) = self.session_cache.get(&key) {
            return Some(LookupHit::Session(v.clone()));
        }
        self.dict.get(text, lang).map(|v| LookupHit::Dict(v.to_string()))
    }

    /// Đồng bộ, KHÔNG gọi mạng. Dịch 1 HTML đã tag (Phần 3) sang `target`.
    /// Cache hit (dict hoặc session) -> thay ngay. Cache miss -> GIỮ NGUYÊN
    /// bản gốc ở lần trả về này; nếu `use_api` bật, đoạn đó được ghi nhận để
    /// dịch nền ở lần gọi spawn_pending_batch() ngay sau đó trong cùng lượt
    /// rebuild_pipeline. Vì vậy đừng ngạc nhiên nếu ngay sau khi thả file,
    /// bản EN/ZH/JA vẫn hiện tạm tiếng Việt — nó tự cập nhật khi API trả kết
    /// quả về (xem poll_background), không cần thao tác gì thêm.
    ///
    /// SỬA LỖI: trước đây tra dict bằng NGUYÊN VĂN đoạn text/attribute-value
    /// (phân biệt hoa/thường, không strip khoảng trắng) — trong khi
    /// local_dict.json (kể cả tự học qua API) luôn dùng key CHỮ THƯỜNG đã
    /// strip, khớp đúng `auto_tagger.py::translate_html_content` (biến
    /// `lower_text`/`lower_val`). Hậu quả: hầu hết mọi tra cứu đều MISS dù
    /// dict đầy đủ tới đâu, vì "SEO Help" không khớp được key "seo help".
    /// Giờ chuẩn hoá qua `normalize_for_lookup` trước khi tra, và khôi phục
    /// lại hoa/thường phù hợp qua `preserve_case`/`apply_hit` sau khi tra
    /// được — cùng 1 quy ước xuyên suốt cache/dict/miss-tracking/API-gọi.
    ///
    /// TÍNH NĂNG MỚI: trả thêm `TranslationStats` (số đoạn khớp từ điển, số
    /// đoạn khớp online, số đoạn còn chưa dịch) — để `rebuild_pipeline` gom
    /// lại qua mọi file/ngôn ngữ, hiện lên UI cho người dùng biết tiến độ rõ
    /// ràng thay vì chỉ có thông báo lỗi rời rạc.
    pub fn translate_html_for_lang(
        &mut self,
        tagged_html: &str,
        target: Language,
        use_api: bool,
    ) -> (String, TranslationStats) {
        let target_code = lang_code(target);
        if target_code == SOURCE_LANG {
            return (tagged_html.to_string(), TranslationStats::default());
        }

        let mut misses: Vec<(String, String)> = Vec::new();
        let mut stats = TranslationStats::default();
        let output = {
            // Reborrow bất biến TƯỜNG MINH: closure chỉ cần ĐỌC self (qua
            // lookup), không cần giữ &mut self — tránh mọi mập mờ về cách
            // closure nắm bắt `self`.
            let this: &Self = self;
            apply::apply_translation(tagged_html, &mut |raw: &str| {
                let Some((trimmed, lower)) = normalize_for_lookup(raw) else {
                    return raw.to_string();
                };
                match this.lookup(&lower, target_code) {
                    Some(LookupHit::Dict(hit)) => {
                        stats.dict_hits += 1;
                        return apply_hit(raw, trimmed, &hit);
                    }
                    Some(LookupHit::Session(hit)) => {
                        stats.online_hits += 1;
                        return apply_hit(raw, trimmed, &hit);
                    }
                    None => {}
                }
                // TÍNH NĂNG MỚI: số điện thoại/email/placeholder La-tinh giả
                // (lorem ipsum...) không đưa vào diện "chưa dịch" thông
                // thường — CHỦ ĐỘNG bỏ qua, không gửi lên API (dịch cũng vô
                // nghĩa hoặc không cần dịch), tiết kiệm hạn mức ít ỏi cho
                // nội dung THẬT SỰ cần dịch. Đếm riêng (skipped_filler) để
                // người dùng thấy rõ đây là quyết định có chủ đích, không
                // phải thiếu sót.
                if should_skip_api(trimmed) {
                    stats.skipped_filler += 1;
                    return raw.to_string();
                }
                stats.untranslated += 1;
                if use_api {
                    misses.push((lower, target_code.to_string()));
                }
                raw.to_string()
            })
        };
        self.misses_this_pass.extend(misses);
        (output, stats)
    }

    /// Gọi SAU khi rebuild_pipeline xử lý xong hết file trong 1 lượt. Gom
    /// miss mới (khử trùng lặp, bỏ qua cái đang in-flight), spawn 1 thread
    /// nền dịch TUẦN TỰ qua API — không dùng async runtime vì khối lượng mỗi
    /// lượt thường nhỏ (vài chục đoạn); tuần tự đơn giản, không block UI vì
    /// chạy trên thread riêng khỏi vòng lặp render.
    pub fn spawn_pending_batch(&mut self) {
        if self.misses_this_pass.is_empty() {
            return;
        }
        let mut batch: Vec<(String, String)> = self.misses_this_pass.drain(..).collect();
        batch.sort();
        batch.dedup();
        batch.retain(|item| self.in_flight.insert(item.clone()));
        if batch.is_empty() {
            return;
        }

        let tx = self.tx.clone();
        let ctx = self.repaint_ctx.clone();
        std::thread::spawn(move || {
            let backend = backend::MyMemoryBackend;
            for (text, lang) in batch {
                // SỬA LỖI: LUÔN gửi tin nhắn qua channel dù thành công hay
                // thất bại (translated=None khi lỗi) — trước đây lỗi thì
                // KHÔNG gửi gì cả, khiến in_flight (chỉ được dọn khi NHẬN
                // được tin nhắn — xem poll_background) không bao giờ được
                // dọn cho item đó, item bị KẸT VĨNH VIỄN trong in_flight suốt
                // phiên làm việc — dù comment cũ ở đây khẳng định "vẫn coi là
                // miss ở lượt sau, tự thử lại", thực tế KHÔNG đúng như vậy.
                // Hậu quả trong thực tế: chỉ cần 1 lần API lỗi/hết hạn mức
                // (rất dễ xảy ra với hạn mức miễn phí ~5000 ký tự/ngày), toàn
                // bộ các đoạn text bị lỗi ở đúng lượt đó sẽ giữ nguyên tiếng
                // Anh mãi mãi cho tới khi khởi động lại app, dù mạng/hạn mức
                // sau đó đã bình thường trở lại.
                let result = backend.translate(&text, SOURCE_LANG, &lang);
                let _ = tx.send(TranslatedItem { text, lang, result });
                if let Some(ctx) = &ctx {
                    ctx.request_repaint();
                }
            }
        });
    }

    /// Gọi mỗi frame — try_recv() không block nên rẻ. Hút hết kết quả nền
    /// đang có, merge vào session_cache + local_dict.json (nếu đủ ngắn) khi
    /// dịch THÀNH CÔNG, dọn khỏi in_flight cho MỌI item (kể cả thất bại —
    /// xem giải thích ở spawn_pending_batch). Trả về true nếu có bản dịch
    /// MỚI THÀNH CÔNG, để caller biết cần rebuild_pipeline lại để áp vào
    /// translated_by_lang; thất bại thì KHÔNG cần rebuild (không có gì đổi
    /// để áp cả), nhưng vẫn phải dọn in_flight để lượt rebuild_pipeline kế
    /// tiếp (do người dùng thao tác gì đó khác) tự động thử lại item đó.
    pub fn poll_background(&mut self) -> bool {
        let mut changed = false;
        let mut dict_dirty = false;
        while let Ok(item) = self.rx.try_recv() {
            // SỬA LỖI CHÍNH: dọn in_flight ở NGOÀI, áp dụng cho MỌI item vừa
            // nhận — kể cả khi thất bại. Trước đây field này chỉ tồn tại khi
            // CHẮC CHẮN thành công, nên vô tình chỉ dọn in_flight cho ca
            // thành công; ca thất bại vẫn gửi được tin nhắn (fix ở
            // spawn_pending_batch) nên giờ CŨNG được dọn ở đây, item đó sẽ
            // tự động là 1 miss bình thường ở lượt rebuild_pipeline kế tiếp,
            // đúng như comment ban đầu đã định.
            self.in_flight.remove(&(item.text.clone(), item.lang.clone()));

            match item.result {
                Ok(translated) => {
                    self.session_cache
                        .insert((item.text.clone(), item.lang.clone()), translated.clone());
                    if should_persist_to_dict(&item.text) {
                        self.dict.insert(&item.text, &item.lang, translated);
                        dict_dirty = true;
                    }
                    changed = true;
                }
                Err(e) => {
                    // Không cache/lưu gì (sẽ tự thử lại sau) — nhưng LƯU LẠI
                    // lý do lỗi để app.rs hiện ra UI, thay vì nuốt mất hoàn
                    // toàn như trước. Chỉ giữ lỗi GẦN NHẤT (đủ dùng để chẩn
                    // đoán, tránh dồn ứ nếu có hàng loạt lỗi liên tiếp).
                    self.last_api_error = Some(e);
                }
            }
        }
        if dict_dirty {
            self.dict.save();
        }
        changed
    }

    /// Lấy VÀ XOÁ lỗi API dịch online gần nhất (nếu có) — gọi mỗi frame từ
    /// app.rs ngay sau `poll_background()` để hiện lên status_message. Dùng
    /// `take()` (không phải đọc trực tiếp field) để mỗi lỗi chỉ "nổi" lên 1
    /// lần, không lặp lại liên tục ở những frame sau khi đã hiện rồi.
    pub fn take_last_api_error(&mut self) -> Option<String> {
        self.last_api_error.take()
    }

    /// true nếu còn ÍT NHẤT 1 đoạn text đã gửi API dịch online nhưng CHƯA có
    /// kết quả trả về (thành công hay thất bại đều chưa biết).
    ///
    /// SỬA LỖI KIẾN TRÚC: bản gốc `auto_tagger.py` dịch qua API HOÀN TOÀN
    /// ĐỒNG BỘ ngay trong lúc Lưu (`save_single_file` gọi thẳng
    /// `translate_html_content`, tự nó gọi `urllib.request.urlopen` chờ từng
    /// request xong mới đi tiếp) — nên thao tác Lưu "chạy lâu" nhưng LUÔN
    /// cho ra file HOÀN CHỈNH. Rust dịch qua API ở 1 thread NỀN riêng (để
    /// không đứng hình UI khi gõ/kéo-thả trong lúc chờ mạng) — nhưng trước
    /// đây KHÔNG có bước nào kiểm tra "còn đang chờ không" trước khi cho
    /// phép Lưu, nên nút Lưu luôn dùng NGAY bất cứ gì đang có trong
    /// `translated_by_lang` tại đúng thời điểm bấm — nếu bấm ngay sau khi
    /// thả file/đổi cấu hình (trước khi thread nền kịp dịch xong), file xuất
    /// ra sẽ lẫn lộn phần đã dịch và phần CÒN NGUYÊN TIẾNG ANH một cách ÂM
    /// THẦM, không cảnh báo gì — đúng triệu chứng "Rust xuất file gần như
    /// ngay tức thì" trong khi Python luôn phải đợi.
    ///
    /// `export::save_all` gọi hàm này để CHẶN lưu (thay vì lưu thiếu trong
    /// im lặng) khi còn đang dịch dở; `ui::config_panel` gọi để tắt (gray
    /// out) nút Lưu, giúp người dùng THẤY NGAY là cần đợi thay vì phải bấm
    /// thử rồi mới biết qua thông báo lỗi.
    pub fn has_pending_translations(&self) -> bool {
        !self.in_flight.is_empty()
    }
}

/// SỬA LỖI: khớp đúng `auto_tagger.py::_should_persist_to_dict` — chỉ lưu
/// xuống local_dict.json những đoạn NGẮN theo SỐ TỪ (1 đến 12 từ), không
/// phải theo SỐ KÝ TỰ (`CACHE_MAX_CHARS` cũ = 60). 2 tiêu chí này KHÔNG
/// tương đương: 1 chuỗi rất nhiều số/ký tự đặc biệt liền nhau (vd URL, mã
/// sản phẩm) có thể dưới 60 ký tự nhưng chỉ tính là "1 từ" theo whitespace,
/// trong khi 1 câu ngắn nhiều từ đơn giản có thể vượt 12 từ dù dưới 60 ký
/// tự — dùng nhầm tiêu chí khiến local_dict.json lưu khác nội dung so với
/// bản Python trong dài hạn (không lộ ra ngay trong 1 lần chạy, nhưng tích
/// luỹ sai khác qua nhiều lần dùng chung 1 file local_dict.json).
fn should_persist_to_dict(text: &str) -> bool {
    let word_count = text.split_whitespace().count();
    (1..=12).contains(&word_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserve_case_matches_python_reference_across_case_patterns() {
        // Đối chiếu trực tiếp với hành vi thật của Python trước khi viết
        // (xem verify_preserve_case.py) — 14 ca, liệt kê lại đây 1 tập con
        // đại diện làm test hồi quy trong Rust.
        assert_eq!(preserve_case("Home", "trang chủ"), "Trang Chủ");
        assert_eq!(preserve_case("HOME", "trang chủ"), "TRANG CHỦ");
        assert_eq!(preserve_case("home", "trang chủ"), "trang chủ");
        assert_eq!(preserve_case("View Cart", "xem giỏ hàng"), "Xem Giỏ Hàng");
        // "SEO Help": "SEO" không phải Title Case hợp lệ (nhiều hơn 1 chữ
        // hoa liên tiếp trong 1 từ) -> rơi xuống nhánh "chỉ viết hoa ký tự
        // đầu", không phải to_title_case.
        assert_eq!(preserve_case("SEO Help", "hỗ trợ seo"), "Hỗ trợ seo");
        assert_eq!(preserve_case("123", "123"), "123");
        assert_eq!(preserve_case("", "gì đó"), "gì đó");
    }

    #[test]
    fn normalize_for_lookup_strips_and_lowercases() {
        assert_eq!(
            normalize_for_lookup("  Home  "),
            Some(("Home", "home".to_string()))
        );
        assert_eq!(normalize_for_lookup("   "), None);
        assert_eq!(normalize_for_lookup(""), None);
    }

    #[test]
    fn apply_hit_preserves_surrounding_whitespace_and_restores_case() {
        // Khoảng trắng đầu/cuối GIỮ NGUYÊN y hệt bản gốc, chỉ phần "lõi" đã
        // strip được thay bằng bản dịch (đã khôi phục hoa/thường).
        assert_eq!(apply_hit("  Home  ", "Home", "trang chủ"), "  Trang Chủ  ");
        assert_eq!(apply_hit("SEO Help", "SEO Help", "hỗ trợ seo"), "Hỗ trợ seo");
        assert_eq!(apply_hit("\tView Cart\n", "View Cart", "xem giỏ hàng"), "\tXem Giỏ Hàng\n");
    }

    #[test]
    fn should_persist_to_dict_counts_words_not_characters() {
        // SỬA LỖI: khớp auto_tagger.py::_should_persist_to_dict (đếm SỐ TỪ,
        // không phải số ký tự — xem giải thích đầy đủ tại định nghĩa hàm).
        assert!(should_persist_to_dict("chat")); // 1 tu
        assert!(should_persist_to_dict("view cart")); // 2 tu
        assert!(should_persist_to_dict("a b c d e f g h i j k l")); // dung 12 tu
        assert!(!should_persist_to_dict("a b c d e f g h i j k l m")); // 13 tu -> qua nguong
        assert!(!should_persist_to_dict("")); // 0 tu -> khong tinh
        assert!(!should_persist_to_dict("   ")); // toan khoang trang -> 0 tu
        // Chuoi 1 "tu" duy nhat nhung rat dai (vd URL/ma san pham) van duoc
        // luu, dung theo tieu chi SO TU cua ban goc chu khong phai so ky tu.
        assert!(should_persist_to_dict(
            "supercalifragilisticexpialidocious-and-then-some-more-extra-characters-here"
        ));
    }

    #[test]
    fn has_pending_translations_reflects_in_flight_set() {
        let mut translator = Translator::new();
        assert!(!translator.has_pending_translations());

        translator.in_flight.insert(("chat".to_string(), "vi".to_string()));
        assert!(translator.has_pending_translations());

        translator.in_flight.remove(&("chat".to_string(), "vi".to_string()));
        assert!(!translator.has_pending_translations());
    }

    #[test]
    fn translate_html_for_lang_reports_dict_online_skipped_and_untranslated_counts() {
        let mut translator = Translator::new();
        // Giả lập: "hello" đã có sẵn trong từ điển offline, "world" vừa dịch
        // thành công qua API TRONG phiên này (session_cache), số điện thoại
        // và đoạn lorem ipsum bị CHỦ ĐỘNG bỏ qua (không tính là miss), còn
        // "untranslated" thì chưa có ở đâu cả và sẽ được ghi nhận là miss.
        translator.dict.insert("hello", "vi", "xin chao".to_string());
        translator
            .session_cache
            .insert(("world".to_string(), "vi".to_string()), "the gioi".to_string());

        let html = r#"<p data-editable="text">Hello</p>
            <p data-editable="text">World</p>
            <p data-editable="text">Untranslated</p>
            <p data-editable="text">+94 423-23-221</p>
            <p data-editable="text">Lorem ipsum dolor sit amet</p>"#;
        let (_, stats) = translator.translate_html_for_lang(html, Language::Vi, true);

        assert_eq!(stats.dict_hits, 1);
        assert_eq!(stats.online_hits, 1);
        assert_eq!(stats.skipped_filler, 2); // so dien thoai + lorem ipsum
        assert_eq!(stats.untranslated, 1);
    }

    #[test]
    fn should_skip_api_catches_phone_email_and_lorem_ipsum_not_real_english() {
        assert!(should_skip_api("+94 423-23-221"));
        assert!(should_skip_api("hi@example.com"));
        assert!(should_skip_api("Lorem ipsum dolor sit amet, consectetur adipiscing elit."));
        assert!(should_skip_api("Nam quis accumsan risus. Aenean id volutpat nibh."));
        // Text tieng Anh that su khong duoc chan nham.
        assert!(!should_skip_api("Chat"));
        assert!(!should_skip_api("View Cart"));
        assert!(!should_skip_api("SEO Help"));
        // Chi 1 tu trung ngau nhien (vd "Duis" cung co the la ten rieng) ->
        // KHONG du nguong 2 tu, khong bi chan nham.
        assert!(!should_skip_api("Duis is a great name for a dog"));
    }

    #[test]
    fn translation_stats_merge_accumulates_across_calls() {
        let mut total = TranslationStats::default();
        total.merge(TranslationStats {
            dict_hits: 3,
            online_hits: 1,
            skipped_filler: 1,
            untranslated: 2,
        });
        total.merge(TranslationStats {
            dict_hits: 1,
            online_hits: 4,
            skipped_filler: 0,
            untranslated: 0,
        });
        assert_eq!(total.dict_hits, 4);
        assert_eq!(total.online_hits, 5);
        assert_eq!(total.skipped_filler, 1);
        assert_eq!(total.untranslated, 2);
    }
}
