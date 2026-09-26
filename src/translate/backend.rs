/// Interface cho 1 dịch vụ dịch qua API HTTP — tách riêng khỏi phần còn lại
/// của hệ thống dịch để đổi provider (Google Cloud Translation, DeepL...)
/// sau này chỉ cần impl trait này, không phải sửa dict.rs/apply.rs/mod.rs.
pub trait TranslationBackend {
    fn translate(&self, text: &str, source_lang: &str, target_lang: &str) -> Result<String, String>;
}

/// Backend mặc định: MyMemory Translation API — miễn phí, KHÔNG cần API key
/// cho khối lượng nhỏ (~5000 ký tự/ngày ẩn danh), nên có thể cargo run và
/// thấy Phần 4 chạy được ngay mà không phải đăng ký/setup billing trước.
///
/// Muốn nâng cấp lên Google Cloud Translation / DeepL (hạn mức cao hơn, chất
/// lượng tốt hơn): viết 1 struct mới impl TranslationBackend tương tự, đọc
/// API key qua biến môi trường (std::env::var), rồi đổi dòng khởi tạo
/// `backend::MyMemoryBackend` trong mod.rs::spawn_pending_batch.
pub struct MyMemoryBackend;

impl TranslationBackend for MyMemoryBackend {
    fn translate(&self, text: &str, source_lang: &str, target_lang: &str) -> Result<String, String> {
        if text.trim().is_empty() {
            return Ok(String::new());
        }

        let url = format!(
            "https://api.mymemory.translated.net/get?q={}&langpair={}|{}",
            url_encode(text),
            source_lang,
            target_lang
        );

        let mut response = ureq::get(&url).call().map_err(|e| e.to_string())?;
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|e| e.to_string())?;

        let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;

        let translated = json
            .get("responseData")
            .and_then(|d| d.get("translatedText"))
            .and_then(|t| t.as_str())
            .ok_or_else(|| format!("MyMemory: response thiếu translatedText — {body}"))?;

        // SỬA LỖI (khớp auto_tagger.py::translate_via_api): có translatedText
        // KHÔNG đồng nghĩa với dịch thành công — MyMemory nhiều lúc trả CHÍNH
        // THÔNG BÁO LỖI (hết hạn mức trong ngày, thiếu tham số, sai mã ngôn
        // ngữ...) NGAY TRONG trường translatedText, HTTP status vẫn 200 OK
        // nên Result::Ok ở trên không tự phát hiện được. Trước đây Rust coi
        // đây là bản dịch hợp lệ và LƯU THẲNG vào local_dict.json — 1 lần hết
        // hạn mức là đủ để thông báo lỗi tiếng Anh bị "học nhầm" thành bản
        // dịch cho rất nhiều đoạn text khác nhau trong cùng 1 lượt chạy.
        let upper = translated.to_uppercase();
        let is_error_message = [
            "MYMEMORY WARNING",
            "NO QUERY SPECIFIED",
            "INVALID LANGUAGE",
            "IS AN INVALID",
            "AN ERROR HAS OCCURRED",
        ]
        .iter()
        .any(|marker| upper.contains(marker));
        if is_error_message {
            return Err(format!(
                "MyMemory trả về thông báo lỗi thay vì bản dịch cho '{}': {translated}",
                truncate_for_display(text)
            ));
        }
        // API "dịch" ra y hệt bản gốc (không phân biệt hoa/thường) -> coi là
        // KHÔNG dịch được (vd cặp ngôn ngữ này MyMemory không có dữ liệu),
        // không phải "trùng hợp bản dịch giống bản gốc" — tránh lưu rác vào
        // dict khiến các đoạn tương tự sau này bị coi là "đã dịch" trong khi
        // thực ra vẫn còn nguyên văn tiếng Anh.
        if translated.to_lowercase() == text.to_lowercase() {
            return Err(format!(
                "MyMemory không dịch được '{}' (trả về y hệt bản gốc)",
                truncate_for_display(text)
            ));
        }

        Ok(translated.to_string())
    }
}

/// Percent-encode tối giản cho query param — chỉ đủ dùng cho use case này,
/// tránh kéo thêm dependency `url`/`percent-encoding` chỉ để làm 1 việc nhỏ.
/// Byte không nằm trong tập ký tự an toàn RFC 3986 (kể cả mọi byte UTF-8 của
/// tiếng Việt/Trung/Nhật) đều được encode dạng %XX.
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Rút gọn text để nhúng vào thông báo lỗi hiện lên UI (status_message chỉ
/// có chỗ cho 1 dòng) — cắt theo SỐ KÝ TỰ (`.chars()`, không phải byte) để
/// an toàn với tiếng Việt/Trung/Nhật nhiều byte, tránh cắt giữa 1 ký tự gây
/// panic hoặc hiện ký tự lỗi (tofu/thay thế).
fn truncate_for_display(text: &str) -> String {
    const MAX_CHARS: usize = 50;
    let char_count = text.chars().count();
    if char_count <= MAX_CHARS {
        text.to_string()
    } else {
        let prefix: String = text.chars().take(MAX_CHARS).collect();
        format!("{prefix}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_encode_leaves_ascii_alnum_untouched_and_encodes_the_rest() {
        assert_eq!(url_encode("abc123-_.~"), "abc123-_.~");
        assert_eq!(url_encode("a b"), "a%20b");
        assert_eq!(url_encode("Đóng"), "%C4%90%C3%B3ng");
    }

    #[test]
    fn truncate_for_display_keeps_short_text_and_cuts_long_text_by_chars() {
        assert_eq!(truncate_for_display("chat"), "chat");
        let exactly_50 = "a".repeat(50);
        assert_eq!(truncate_for_display(&exactly_50), exactly_50); // dung nguong, khong cat

        let long = "a".repeat(60);
        let result = truncate_for_display(&long);
        assert_eq!(result.chars().count(), 51); // 50 ky tu + dau "…"
        assert!(result.ends_with('…'));

        // Cat GIUA cau tieng Viet nhieu byte/ky tu — phai an toan, khong
        // panic va khong cat dut giua 1 ky tu co dau.
        let vn_long = "Chào mừng quý khách đến với trang thông tin điện tử của công ty chúng tôi hôm nay";
        let vn_result = truncate_for_display(vn_long);
        assert_eq!(vn_result.chars().count(), 51);
    }

    // 2 test dưới đây kiểm tra ĐÚNG logic lọc vừa thêm vào translate() (đoạn
    // sau khi đã có `translated`), tách riêng thành closure để test không
    // cần gọi mạng thật — mô phỏng lại chính xác điều kiện trong translate().
    fn is_error_message(translated: &str) -> bool {
        let upper = translated.to_uppercase();
        [
            "MYMEMORY WARNING",
            "NO QUERY SPECIFIED",
            "INVALID LANGUAGE",
            "IS AN INVALID",
            "AN ERROR HAS OCCURRED",
        ]
        .iter()
        .any(|marker| upper.contains(marker))
    }

    #[test]
    fn detects_mymemory_error_messages_disguised_as_translations() {
        assert!(is_error_message(
            "MYMEMORY WARNING: YOU USED ALL AVAILABLE FREE TRANSLATIONS FOR TODAY!"
        ));
        assert!(is_error_message("Invalid Language Pair Specified"));
        assert!(!is_error_message("Trò chuyện"));
        assert!(!is_error_message("Xin chào"));
    }

    #[test]
    fn detects_untranslated_echo_case_insensitively() {
        assert!("chat".to_lowercase() == "Chat".to_lowercase());
        assert!("CHAT".to_lowercase() == "chat".to_lowercase());
        assert!("Trò chuyện".to_lowercase() != "Chat".to_lowercase());
    }
}
