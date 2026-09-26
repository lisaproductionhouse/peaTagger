use std::collections::HashSet;
use std::sync::LazyLock;

/// Số chữ số tối thiểu để 1 chuỗi được coi là "trông giống số điện thoại".
/// 6 đủ thấp để bắt các hotline ngắn (vd "1900 6750" dạng SĐT tổng đài VN)
/// nhưng đủ cao để KHÔNG nhầm với năm (4 chữ số) hay mã bưu điện ngắn.
const MIN_PHONE_DIGITS: usize = 6;

/// Từ "chỉ điểm" Lorem Ipsum — CHỈ chọn từ không trùng bất kỳ từ/viết tắt
/// tiếng Anh thông dụng nào, dù chúng CÓ mặt trong đoạn Lorem Ipsum kinh
/// điển — cố tình loại "sit", "do", "id", "est" (viết tắt "established"),
/// "non", "qui", "ex", "in", "ad"... vì đây đều là từ/viết tắt tiếng Anh
/// thật, dễ báo sai trên nội dung thật (vd "Please sit down", "Est. 2020").
///
/// Gộp CẢ bộ từ vựng La-tinh MỞ RỘNG hay gặp trong theme HTML bán sẵn (vd
/// ThemeForest) — KHÔNG chỉ đúng đoạn "Lorem ipsum dolor sit amet" kinh
/// điển — vì các đoạn nối tiếp trong 1 khối nhiều câu thường KHÔNG lặp lại
/// đúng 2 từ "lorem"/"ipsum" nữa. Ca thực tế: "Nam quis accumsan risus.
/// Aenean id volutpat nibh," — không có "lorem"/"ipsum" nhưng vẫn khớp qua
/// accumsan/aenean/volutpat/nibh (thuộc bộ mở rộng).
///
/// Chỉ cần khớp ĐÚNG 1 từ (theo ranh giới từ qua tokenize, không phải dò
/// substring trên cả chuỗi — "elite" sẽ KHÔNG khớp "elit") là đủ báo hiệu —
/// các từ này gần như không thể xuất hiện tình cờ trong nội dung tiếng Anh
/// thật đang chờ dịch.
///
/// Muốn bổ sung/bớt từ: sửa trực tiếp mảng này, không cần đổi logic phía
/// dưới.
static LOREM_IPSUM_WORDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        // Đoạn kinh điển "Lorem ipsum dolor sit amet..."
        "lorem", "ipsum", "dolor", "amet", "consectetur", "adipiscing", "elit",
        "eiusmod", "tempor", "incididunt", "labore", "dolore", "magna", "aliqua",
        "enim", "minim", "veniam", "quis", "nostrud", "exercitation", "ullamco",
        "laboris", "nisi", "aliquip", "commodo", "consequat", "duis", "aute",
        "irure", "reprehenderit", "voluptate", "velit", "esse", "cillum",
        "fugiat", "nulla", "pariatur", "excepteur", "sint", "occaecat",
        "cupidatat", "proident", "culpa", "officia", "deserunt", "mollit",
        "anim", "laborum",
        // Bộ mở rộng hay gặp trong theme HTML bán sẵn (ThemeForest...) — nơi
        // 1 khối nhiều câu được sinh ra, chỉ câu ĐẦU mới có "lorem ipsum".
        "vestibulum", "curabitur", "etiam", "quisque", "pellentesque",
        "phasellus", "vivamus", "suspendisse", "praesent", "maecenas",
        "accumsan", "aenean", "volutpat", "nibh", "condimentum", "ultricies",
        "sagittis", "sodales", "vulputate", "hendrerit", "molestie", "gravida",
        "mattis", "eleifend", "convallis", "tincidunt", "posuere", "cubilia",
        "nascetur", "ridiculus", "torquent", "conubia", "himenaeos",
        "fringilla", "malesuada", "faucibus", "luctus", "ultrices",
        "habitasse", "platea", "dictumst", "egestas", "rhoncus", "porttitor",
        "lacinia", "elementum", "feugiat", "scelerisque", "ligula", "dapibus",
        "blandit", "interdum", "lobortis", "imperdiet", "congue", "auctor",
        "aptent", "taciti", "sociosqu", "litora", "inceptos", "fusce",
        "sapien", "mauris", "tortor", "tellus", "parturient", "sollicitudin",
        "tristique", "venenatis", "euismod", "dignissim", "nunc", "morbi",
        "risus", "urna", "orci", "arcu",
    ]
    .into_iter()
    .collect()
});

/// True nếu `text` là placeholder KHÔNG NÊN đưa qua bước dịch (cả tra
/// local_dict.json LẪN gọi API online) — đoạn Lorem Ipsum, hoặc 1 chuỗi
/// trông giống số điện thoại/fax giả. Gọi hàm này ở ĐIỂM ĐẦU của bước
/// resolve 1 chuỗi text/attribute, TRƯỚC cả tra dict lẫn enqueue API — mục
/// tiêu là để nội dung này giữ NGUYÊN VĂN, không phải "dịch cho tiết kiệm
/// hạn mức", vì nó chắc chắn sẽ bị thay bằng nội dung thật trước khi lên
/// production.
///
/// Chỉ là heuristic: không đảm bảo bắt hết 100% (1 dạng lorem ipsum sinh
/// chữ hiếm ngoài danh sách vẫn lọt qua được), và có xác suất RẤT NHỎ báo
/// sai trên nội dung thật (xem chú thích ở LOREM_IPSUM_WORDS) — đây là lý do
/// nên có tuỳ chọn bật/tắt ở ConfigState thay vì hard-code luôn bật.
pub fn is_untranslatable_placeholder(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    looks_like_phone_number(trimmed) || contains_lorem_ipsum_word(trimmed)
}

/// Toàn bộ ký tự (sau trim) chỉ gồm chữ số + ký tự định dạng SĐT thường gặp
/// (khoảng trắng, +, -, (, ), .) VÀ có ít nhất MIN_PHONE_DIGITS chữ số —
/// bắt được các định dạng phổ biến ("+(305) 222-3333", "1-800-555-0199",
/// "084.123.4567"...) mà không nhầm với năm/mã bưu điện ngắn, và không nhầm
/// với chuỗi có LẪN chữ cái (vd địa chỉ "123 Main St" vẫn cần dịch bình
/// thường vì còn "Main St" là nội dung thật).
fn looks_like_phone_number(text: &str) -> bool {
    let digit_count = text.chars().filter(char::is_ascii_digit).count();
    if digit_count < MIN_PHONE_DIGITS {
        return false;
    }
    text.chars()
        .all(|c| c.is_ascii_digit() || matches!(c, ' ' | '+' | '-' | '(' | ')' | '.'))
}

/// Tách `text` thành từng từ theo ranh giới KÝ TỰ KHÔNG PHẢI chữ/số (an toàn
/// với dấu câu, xuống dòng...), so khớp CHÍNH XÁC (không phải substring) với
/// LOREM_IPSUM_WORDS — nhờ vậy "elite" không bị nhầm thành khớp "elit".
fn contains_lorem_ipsum_word(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| !word.is_empty() && LOREM_IPSUM_WORDS.contains(word))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_classic_lorem_ipsum_opening() {
        assert!(is_untranslatable_placeholder(
            "Lorem ipsum dolor sit amet, consectetur adipiscing elit."
        ));
    }

    #[test]
    fn detects_extended_lorem_ipsum_without_the_words_lorem_or_ipsum() {
        // Ca thực tế gặp phải: câu nối tiếp trong 1 khối nhiều câu, không
        // chứa "lorem"/"ipsum" nhưng vẫn là placeholder vô nghĩa.
        assert!(is_untranslatable_placeholder(
            "Nam quis accumsan risus. Aenean id volutpat nibh,"
        ));
    }

    #[test]
    fn detects_phone_numbers_in_various_formats() {
        assert!(is_untranslatable_placeholder("+(305) 222-3333"));
        assert!(is_untranslatable_placeholder("1-800-555-0199"));
        assert!(is_untranslatable_placeholder("084.123.4567"));
    }

    #[test]
    fn does_not_flag_real_content() {
        assert!(!is_untranslatable_placeholder("Contact us today"));
        assert!(!is_untranslatable_placeholder("About our company"));
        // "Nam" CỐ TÌNH không có trong danh sách — trùng tên riêng tiếng Việt
        // rất phổ biến (vd "Nguyễn Văn Nam"), khác với "nam" trong Lorem Ipsum
        // mở rộng vốn luôn đi kèm các từ chỉ điểm khác (đã bắt ở test trên).
        assert!(!is_untranslatable_placeholder("Nam is our lead designer"));
        assert!(!is_untranslatable_placeholder("Suite 400, 123 Main St"));
        assert!(!is_untranslatable_placeholder("Est. 2020"));
    }

    #[test]
    fn does_not_flag_short_numbers_like_zip_or_year() {
        assert!(!is_untranslatable_placeholder("10001"));
        assert!(!is_untranslatable_placeholder("2024"));
    }

    #[test]
    fn empty_or_whitespace_is_not_flagged() {
        assert!(!is_untranslatable_placeholder(""));
        assert!(!is_untranslatable_placeholder("   "));
    }
}
