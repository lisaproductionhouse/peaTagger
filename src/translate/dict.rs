use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Đường dẫn tới local_dict.json.
///
/// Rust không có khái niệm tương đương `sys.frozen` của PyInstaller — không
/// thể check runtime "đang chạy qua interpreter hay đã đóng gói" như Python.
/// Nhưng Rust CÓ 1 thứ tốt hơn cho đúng nhu cầu này: `debug_assertions` —
/// biết CHẮC CHẮN ngay lúc COMPILE đây là bản debug (`cargo run`/
/// `cargo build`) hay release (`cargo build --release`, đúng bản người dùng
/// thật sự chạy) — không cần đoán qua runtime.
///
/// - Bản RELEASE: LUÔN dùng exe-relative (`current_exe()`) DỨT KHOÁT, không
///   check CWD trước — đây là bản người dùng thật sự chạy, phải khớp đúng
///   `.exe` thực tế bất kể CWD lúc khởi chạy (shortcut ghim taskbar, launcher
///   khác...), giống hệt `sys.executable` của auto_tagger.py khi đã đóng gói.
/// - Bản DEBUG (`cargo run`): current_exe() trỏ vào `target/debug/...`,
///   KHÔNG phải gốc project nơi local_dict.json thật sự nằm khi dev — ưu
///   tiên CWD trước (thường là gốc project lúc `cargo run`) NẾU đã có sẵn
///   file ở đó, fallback về exe-relative nếu không.
///
/// LƯU Ý QUAN TRỌNG: KHÔNG dùng "CWD nếu tồn tại, dù bản nào" cho CẢ 2
/// trường hợp — vì bản RELEASE mà ưu tiên CWD sẽ tạo ra rủi ro MỚI: nếu bất
/// kỳ thư mục nào .exe được khởi chạy từ đó (kể cả do shortcut đặt CWD ở nơi
/// không ngờ) TÌNH CỜ có sẵn 1 file tên "local_dict.json" (sót lại từ lần
/// test khác, project khác...), app sẽ ÂM THẦM dùng NHẦM file đó (thậm chí
/// GHI ĐÈ học được vào nhầm chỗ khi lưu) — khó debug hơn nhiều so với lỗi
/// CWD-only cũ (vốn ít nhất luôn thất bại theo CÙNG 1 kiểu, dễ đoán).
/// `#[cfg(debug_assertions)]` tránh được rủi ro này vì nó QUYẾT ĐỊNH DỨT
/// KHOÁT lúc compile, không phụ thuộc "tình cờ có file hay không" lúc chạy.
fn dict_path() -> PathBuf {
    let exe_relative = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("local_dict.json")))
        .unwrap_or_else(|| PathBuf::from("local_dict.json"));

    resolve_dict_path(cfg!(debug_assertions), Path::new("local_dict.json").exists(), exe_relative)
}

/// Logic THUẦN đằng sau `dict_path()` — nhận đầu vào qua THAM SỐ thay vì tự
/// đọc thẳng `cfg!`/filesystem, để test được đầy đủ cả 3 nhánh mà KHÔNG phải
/// đổi CWD thật của tiến trình test. Đổi CWD bằng `std::env::set_current_dir`
/// trong test rất dễ gây nhiễu chéo: CWD là trạng thái TOÀN CỤC của cả tiến
/// trình, trong khi `cargo test` mặc định chạy nhiều test SONG SONG trong
/// CÙNG 1 tiến trình — 1 test đổi CWD có thể làm sai lệch kết quả của test
/// khác đang chạy đồng thời.
fn resolve_dict_path(is_debug_build: bool, cwd_file_exists: bool, exe_relative: PathBuf) -> PathBuf {
    if is_debug_build && cwd_file_exists {
        PathBuf::from("local_dict.json")
    } else {
        exe_relative
    }
}

/// Từ điển cục bộ: text gốc -> {mã ngôn ngữ -> bản dịch}.
///
/// Dùng HashMap lồng nhau thay vì struct cố định { en, vi, zh, ja } vì spec
/// yêu cầu "khả năng mở rộng ngôn ngữ khác" — thêm ngôn ngữ mới chỉ là thêm
/// 1 key trong JSON, code KHÔNG cần đổi. Lookup O(1) trên cả 2 trục (text,
/// ngôn ngữ) — đáp ứng "tối ưu, thời gian xử lý nhanh". Serialize/deserialize
/// thẳng kiểu HashMap<String, HashMap<String,String>> (không qua struct
/// riêng có derive) để JSON trên đĩa đúng y hệt shape mong đợi:
/// { "Liên hệ": { "en": "Contact", "zh-CN": "联系我们" }, ... }
///
/// LƯU Ý: key cấp 1 (text gốc) PHẢI đã là chữ thường — xem
/// translate/mod.rs::translate_html_for_lang, nơi tra/lưu dict đều chuẩn hoá
/// lowercase trước khi gọi get()/insert() ở đây; struct này chỉ lưu trữ
/// thuần tuý, không tự lowercase hộ.
pub struct LocalDict {
    entries: HashMap<String, HashMap<String, String>>,
}

impl LocalDict {
    /// Trả về (dict, thông báo lỗi nếu file JSON tồn tại nhưng cú pháp sai).
    /// File CHƯA TỒN TẠI (lần chạy đầu tiên, chưa có gì để học) không tính
    /// là lỗi — chỉ khi file CÓ MẶT nhưng serde_json không parse được (vd dư
    /// dấu phẩy cuối, thiếu ngoặc...) mới báo, vì lúc đó người dùng có khả
    /// năng đã tự tay soạn/sửa file và cần biết để sửa lại.
    pub fn load() -> (Self, Option<String>) {
        let path = dict_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str(&text) {
                Ok(entries) => (Self { entries }, None),
                Err(e) => (
                    Self {
                        entries: HashMap::new(),
                    },
                    Some(format!(
                        "{} có lỗi cú pháp JSON nên bị bỏ qua (đã bắt đầu từ điển rỗng): {e}",
                        path.display()
                    )),
                ),
            },
            Err(_) => (
                Self {
                    entries: HashMap::new(),
                },
                None,
            ),
        }
    }

    /// Lỗi ghi file (disk đầy, không có quyền...) bị bỏ qua có chủ đích:
    /// dict chỉ là CACHE, mất bản ghi mới nhất không làm hỏng dữ liệu gì —
    /// lần chạy sau sẽ tự học lại qua API. Không đáng để phá luồng UI vì lỗi
    /// ghi file phụ trợ này.
    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(&self.entries) {
            let _ = std::fs::write(dict_path(), json);
        }
    }

    pub fn get(&self, text: &str, lang: &str) -> Option<&str> {
        self.entries.get(text)?.get(lang).map(String::as_str)
    }

    pub fn insert(&mut self, text: &str, lang: &str, translation: String) {
        self.entries
            .entry(text.to_string())
            .or_default()
            .insert(lang.to_string(), translation);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_build_prefers_cwd_file_when_it_exists() {
        let exe_relative = PathBuf::from("/exe/dir/local_dict.json");
        let result = resolve_dict_path(true, true, exe_relative);
        assert_eq!(result, PathBuf::from("local_dict.json"));
    }

    #[test]
    fn debug_build_falls_back_to_exe_relative_when_cwd_file_missing() {
        let exe_relative = PathBuf::from("/exe/dir/local_dict.json");
        let result = resolve_dict_path(true, false, exe_relative.clone());
        assert_eq!(result, exe_relative);
    }

    #[test]
    fn release_build_always_uses_exe_relative_even_if_cwd_file_exists() {
        // Đây là thuộc tính AN TOÀN CỐT LÕI của toàn bộ thiết kế: bản
        // RELEASE (is_debug_build=false) phải LUÔN dùng exe-relative, DÙ
        // CWD có sẵn 1 file tên "local_dict.json" đi chăng nữa (`true`) —
        // không được để lộ cơ hội "tình cờ dùng nhầm file khác ở CWD" ra
        // bản người dùng thật sự chạy.
        let exe_relative = PathBuf::from("/exe/dir/local_dict.json");
        let result = resolve_dict_path(false, true, exe_relative.clone());
        assert_eq!(result, exe_relative);
    }

    #[test]
    fn get_returns_none_when_missing_and_some_when_present() {
        let mut dict = LocalDict {
            entries: HashMap::new(),
        };
        assert_eq!(dict.get("Liên hệ", "en"), None);

        dict.insert("Liên hệ", "en", "Contact".to_string());
        assert_eq!(dict.get("Liên hệ", "en"), Some("Contact"));
        assert_eq!(dict.get("Liên hệ", "zh-CN"), None);
    }

    #[test]
    fn round_trips_through_json_shape_without_wrapper_key() {
        let mut dict = LocalDict {
            entries: HashMap::new(),
        };
        dict.insert("Đóng", "en", "Close".to_string());
        let json = serde_json::to_string(&dict.entries).unwrap();
        // Không có key bọc ngoài kiểu {"entries": ...} — text gốc là key cấp 1.
        assert!(json.contains(r#""Đóng":{"en":"Close"}"#));
    }
}
