use eframe::egui;
use scraper::{Html, Node, Selector};
use similar::{ChangeTag, TextDiff};
use std::ops::Range;
use std::sync::LazyLock;

/// So sánh `old` (code gốc) với `new` (bản còn lại — đã tag/đã dịch/pending)
/// ở cả cấp DÒNG và TỪ, đồng thời GIÓNG HÀNG 2 cột theo CẢ 2 CHIỀU:
///
/// 1) GIÓNG THEO SỐ DÒNG: dòng nào chỉ có ở 1 bên (bị xoá hẳn / thêm hẳn,
///    không có dòng tương ứng bên kia) được bù 1 dòng TRỐNG bên còn lại ở
///    ĐÚNG vị trí đó — xem thuật toán chi tiết ở `flush_block` bên dưới.
/// 2) GIÓNG THEO CHIỀU CAO (không xuống dòng — word-wrap): 1 dòng dài hơn
///    bên này (vd sau khi gắn thêm data-builder-id/data-editable) trước đây
///    tự XUỐNG DÒNG (word-wrap) trong khung xem, làm chiều cao hiển thị của
///    RIÊNG dòng đó cao hơn bên kia — mọi dòng phía dưới bị đẩy lệch dần dù
///    số DÒNG LOGIC đã khớp. Bản chất 2 mục tiêu "tự xuống dòng cho vừa
///    khung" và "mỗi dòng logic cao bằng nhau ở cả 2 bên" mâu thuẫn nhau —
///    không thể vừa tự ngắt dòng theo bề rộng khung VỪA đảm bảo chiều cao
///    từng dòng luôn bằng nhau ở 2 bên (bề rộng khung 2 bên bằng nhau,
///    nhưng ĐỘ DÀI dòng ở 2 bên khác nhau, nên nếu tự ngắt dòng thì SỐ DÒNG
///    HIỂN THỊ ra sau khi ngắt của cùng 1 dòng logic hoàn toàn có thể khác
///    nhau giữa 2 bên). Cách xử lý CHẮC CHẮN đúng, không cần đo/đoán chiều
///    cao: TẮT xuống dòng tự động (đặt `wrap.max_width` = vô cực ở cuối hàm
///    này) — dòng dài thì tràn ngang, cuộn ngang để xem hết (xem
///    `ScrollArea::both()` ở preview.rs), CHỨ KHÔNG tự ngắt. Nhờ vậy 1 dòng
///    logic LUÔN chiếm ĐÚNG 1 dòng hiển thị ở CẢ 2 bên, không có sai số.
///
/// SỬA LỖI: trước đây (bản đầu) 2 cột chỉ nối các dòng CỦA RIÊNG MÌNH lại
/// liên tục — dòng bị xoá chỉ hiện bên trái, dòng thêm mới chỉ hiện bên
/// phải, không bù chỗ trống cho bên kia (mục 1 ở trên chưa có). Sau khi sửa
/// mục 1, người dùng phát hiện vẫn lệch — do mục 2 (word-wrap) gây ra, một
/// nguyên nhân HOÀN TOÀN KHÁC, không liên quan tới thuật toán diff mà liên
/// quan tới cách hiển thị.
///
/// Dùng crate `similar` (Myers/Patience diff — nền tảng của insta, ruff...)
/// thay vì tự viết thuật toán diff: đây là bài toán đã có lời giải tốt, kiểm
/// thử kỹ trong cả ecosystem Rust, tự viết lại chỉ thêm rủi ro mà không có
/// lợi ích gì thêm — nhất là khi không compile-check được.
///
/// `iter_all_inline_changes()` làm diff dòng trước, rồi tự tinh chỉnh thêm 1
/// lớp diff TỪ bên trong các dòng bị THAY THẾ (không phải thêm/xoá nguyên
/// dòng) — cờ `emphasized` cho biết từ nào thực sự khác nhau. Nhờ vậy 1 dòng
/// chỉ sửa vài từ sẽ không bị tô đỏ/xanh nguyên dòng, chỉ đúng phần đổi.
///
/// THUẬT TOÁN GIÓNG HÀNG (mục 1): `iter_all_inline_changes()` trả về 1 luồng
/// phẳng, nhưng với 1 khối "thay thế" (vd 3 dòng cũ đổi thành 1 dòng mới),
/// theo đúng hành vi đã xác nhận qua ví dụ chính thức của crate `similar`
/// (doc `iter_all_changes`), nó LUÔN trả hết các dòng Delete của khối đó rồi
/// mới tới các dòng Insert — không xen kẽ. Nhờ vậy chỉ cần GOM các dòng
/// Delete/Insert LIÊN TIẾP (giữa 2 mốc Equal) thành 1 khối, rồi trong khối
/// đó ghép dòng xoá thứ i với dòng thêm thứ i THEO VỊ TRÍ (phần tô đậm
/// từ-đổi-gì đã do chính iter_all_inline_changes() quyết định sẵn, ở đây chỉ
/// cần giữ đúng thứ tự nó trả về, không tự chấm điểm lại độ giống nhau). Dư
/// ra bên nào thì bù DÒNG TRỐNG cho bên thiếu ở đúng những vị trí dư đó —
/// xem `flush_block`.
///
/// `colorize`: `true` = tô ĐỎ phần xoá / XANH phần thêm (chế độ "Hiện diff");
/// `false` = vẫn gióng hàng y hệt nhưng giữ màu mặc định, không tô (dùng cho
/// mọi cặp Code gốc <-> [Đã tag/EN/VI/ZH/JA] — những cặp này cũng lệch dòng
/// vì html5ever tổ chức lại xuống dòng khi parse+serialize lại, không chỉ
/// riêng chế độ diff mới cần gióng hàng).
///
/// Trả về (job_trái, job_phải): CÙNG SỐ DÒNG, CÙNG KHÔNG XUỐNG DÒNG TỰ ĐỘNG.
pub fn build_diff_jobs(old: &str, new: &str) -> (egui::text::LayoutJob, egui::text::LayoutJob) {
    build_aligned_jobs(old, new, true)
}

/// Như `build_diff_jobs`, nhưng KHÔNG tô đỏ/xanh — dùng cho các cặp so sánh
/// không phải chế độ "Hiện diff" (Đã tag/EN/VI/ZH/JA so với Code gốc) vẫn
/// cần gióng hàng dù không cần màu.
pub fn build_aligned_plain_jobs(old: &str, new: &str) -> (egui::text::LayoutJob, egui::text::LayoutJob) {
    build_aligned_jobs(old, new, false)
}

fn build_aligned_jobs(old: &str, new: &str, colorize: bool) -> (egui::text::LayoutJob, egui::text::LayoutJob) {
    let diff = TextDiff::from_lines(old, new);

    let font = egui::FontId::monospace(13.0);
    let default_color = egui::Color32::from_gray(220);
    let (red, green) = if colorize {
        (egui::Color32::from_rgb(255, 110, 110), egui::Color32::from_rgb(120, 220, 140))
    } else {
        (default_color, default_color)
    };

    let mut left = egui::text::LayoutJob::default();
    let mut right = egui::text::LayoutJob::default();

    // Mỗi phần tử = các đoạn (emphasized, text) của ĐÚNG 1 dòng. Gom lại chờ
    // hết khối rồi mới ghép+bù ở flush_block(), vì phải thấy dòng KẾ TIẾP
    // (Equal hoặc hết luồng) mới biết khối đó dừng ở đâu, dài bao nhiêu.
    let mut pending_deletes: Vec<Vec<(bool, String)>> = Vec::new();
    let mut pending_inserts: Vec<Vec<(bool, String)>> = Vec::new();

    for change in diff.iter_all_inline_changes() {
        match change.tag() {
            ChangeTag::Equal => {
                flush_block(&mut left, &mut right, &mut pending_deletes, &mut pending_inserts, &font, default_color, red, green);
                for (_, piece) in change.iter_strings_lossy() {
                    left.append(&piece, 0.0, text_format(&font, default_color));
                    right.append(&piece, 0.0, text_format(&font, default_color));
                }
            }
            ChangeTag::Delete => {
                let segments = change
                    .iter_strings_lossy()
                    .map(|(emphasized, text)| (emphasized, text.into_owned()))
                    .collect();
                pending_deletes.push(segments);
            }
            ChangeTag::Insert => {
                let segments = change
                    .iter_strings_lossy()
                    .map(|(emphasized, text)| (emphasized, text.into_owned()))
                    .collect();
                pending_inserts.push(segments);
            }
        }
    }
    // Luồng có thể KẾT THÚC ngay giữa 1 khối Delete/Insert (vd file mới toàn
    // bộ là dòng thêm mới, không có mốc Equal nào theo sau) — xả nốt phần
    // còn lại, nếu không sẽ mất trắng những dòng cuối cùng đó.
    flush_block(&mut left, &mut right, &mut pending_deletes, &mut pending_inserts, &font, default_color, red, green);

    // TẮT xuống dòng tự động — xem mục (2) ở doc comment của hàm này. Đặt Ở
    // ĐÂY (không phải ở preview.rs) để mọi nơi gọi build_diff_jobs/
    // build_aligned_plain_jobs đều tự động được áp dụng, không cần nhớ set
    // lại mỗi chỗ gọi.
    left.wrap.max_width = f32::INFINITY;
    right.wrap.max_width = f32::INFINITY;

    (left, right)
}

fn text_format(font: &egui::FontId, color: egui::Color32) -> egui::text::TextFormat {
    egui::text::TextFormat {
        font_id: font.clone(),
        color,
        ..Default::default()
    }
}

/// Ghi 1 dòng (đã tách sẵn từng đoạn emphasized/không) vào `job`, dùng
/// `emphasized_color` cho phần tô đậm, `default_color` cho phần còn lại.
fn append_line(
    job: &mut egui::text::LayoutJob,
    segments: &[(bool, String)],
    font: &egui::FontId,
    default_color: egui::Color32,
    emphasized_color: egui::Color32,
) {
    for (emphasized, text) in segments {
        let color = if *emphasized { emphasized_color } else { default_color };
        job.append(text, 0.0, text_format(font, color));
    }
}

/// 1 dòng trống thuần tuý — dùng để bù chỗ cho bên không có dòng tương ứng,
/// giữ số dòng 2 cột khớp nhau qua khối này.
fn append_blank_line(job: &mut egui::text::LayoutJob, font: &egui::FontId) {
    job.append("\n", 0.0, text_format(font, egui::Color32::TRANSPARENT));
}

/// Ghép cặp các dòng đã gom trong 1 khối Delete/Insert liên tiếp THEO VỊ TRÍ
/// (dòng xoá thứ i <-> dòng thêm thứ i), bù dòng TRỐNG cho bên có ít dòng
/// hơn ở đúng những vị trí dư — xem giải thích thuật toán ở doc comment của
/// `build_diff_jobs`. Dọn sạch 2 buffer sau khi ghi xong để sẵn sàng cho
/// khối kế tiếp.
#[allow(clippy::too_many_arguments)]
fn flush_block(
    left: &mut egui::text::LayoutJob,
    right: &mut egui::text::LayoutJob,
    pending_deletes: &mut Vec<Vec<(bool, String)>>,
    pending_inserts: &mut Vec<Vec<(bool, String)>>,
    font: &egui::FontId,
    default_color: egui::Color32,
    red: egui::Color32,
    green: egui::Color32,
) {
    let row_count = pending_deletes.len().max(pending_inserts.len());
    for i in 0..row_count {
        match pending_deletes.get(i) {
            Some(segments) => append_line(left, segments, font, default_color, red),
            None => append_blank_line(left, font),
        }
        match pending_inserts.get(i) {
            Some(segments) => append_line(right, segments, font, default_color, green),
            None => append_blank_line(right, font),
        }
    }
    pending_deletes.clear();
    pending_inserts.clear();
}

/// Selector tĩnh y hệt `translate/apply.rs::TAGGED_SELECTOR` — khai báo lại
/// riêng ở đây (thay vì import từ module `translate`, vốn là `mod` riêng
/// không public các item nội bộ) vì đây là diff/hiển thị, không phải bước
/// dịch thật; trùng lặp 1 chuỗi selector tĩnh chấp nhận được.
static TAGGED_SELECTOR: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("[data-editable]").expect("selector tĩnh hợp lệ"));

/// 5 thuộc tính có thể dịch — ĐÚNG THỨ TỰ như `apply.rs::TRANSLATABLE_ATTRS`
/// (thứ tự này quyết định thứ tự các "đơn vị dịch" xuất hiện trong danh sách
/// trả về của `extract_translation_units`, PHẢI khớp cả 2 bên tagged/dịch).
const TRANSLATABLE_ATTRS: [&str; 5] = ["placeholder", "alt", "title", "content", "value"];

/// So sánh 2 bản ĐÃ GẮN TAG — bản TRƯỚC dịch (`tagged_html`, vd
/// `pending_html`) và bản SAU dịch 1 ngôn ngữ cụ thể (`translated_html`) —
/// tô màu theo TỪNG ĐOẠN text node/thuộc tính có thể dịch, KHÔNG theo dòng
/// như `build_diff_jobs`/`build_aligned_plain_jobs` ở trên (2 hàm đó diff
/// CẤU TRÚC DÒNG; đây là highlight NỘI DUNG DỊCH — bản chất khác hẳn). Không
/// cần thuật toán diff dòng ở đây: apply.rs (Phần 4) không bao giờ thêm/xoá/
/// di chuyển node khi dịch, chỉ đổi text bên trong node/thuộc tính đã có sẵn
/// — nên 2 bản LUÔN cùng cấu trúc dòng tuyệt đối, không có gì để "gióng
/// hàng" ở cấp dòng cả.
///
/// CÁCH XÁC ĐỊNH "phần nào là 1 đơn vị có thể dịch": lặp lại CHÍNH XÁC logic
/// duyệt cây của `apply.rs::apply_translation` (cùng selector
/// `[data-editable]`, cùng 5 thuộc tính `TRANSLATABLE_ATTRS` theo ĐÚNG thứ
/// tự, cùng quy tắc "chỉ con TRỰC TIẾP mới là text node được dịch") trên CẢ
/// 2 văn bản. Vì cấu trúc cây giống hệt nhau (xem trên), đơn vị thứ i thu
/// được từ bản tagged và đơn vị thứ i từ bản dịch LUÔN ứng với CÙNG 1 vị trí
/// trong cây — khớp cặp với nhau THEO CHỈ SỐ, không cần so khớp nội dung.
///
/// CÁCH TÌM VỊ TRÍ BYTE trong chuỗi đã serialize: scraper/html5ever không lộ
/// offset của node trong chuỗi `.html()` trả về, nên với MỖI đơn vị, tìm
/// chuỗi con (đã escape `&`/`<`/`"` giống quy tắc serialize của html5ever)
/// bắt đầu tìm từ VỊ TRÍ TÌM THẤY LẦN TRƯỚC (không phải từ đầu chuỗi mỗi
/// lần) — vì các đơn vị luôn xuất hiện trong chuỗi ĐÚNG THEO THỨ TỰ duyệt
/// cây, tìm tiến dần tuần tự là đủ chính xác, không cần viết lại serializer.
/// Nếu 1 đơn vị hiếm khi KHÔNG tìm thấy (vd quy tắc escape lệch ở 1 ca đặc
/// biệt chưa lường hết) thì BỎ QUA tô màu riêng đơn vị đó (giữ màu mặc định)
/// thay vì làm hỏng cả panel — không panic, không lệch dây chuyền sang các
/// đơn vị sau.
pub fn build_translation_highlight_jobs(
    tagged_html: &str,
    translated_html: &str,
) -> (egui::text::LayoutJob, egui::text::LayoutJob) {
    let font = egui::FontId::monospace(13.0);
    let default_color = egui::Color32::from_gray(220);
    let attr_color = egui::Color32::from_gray(115);
    let source_color = egui::Color32::from_rgb(140, 180, 215);
    let translated_color = egui::Color32::from_rgb(150, 200, 160);
    let warn_text = egui::Color32::from_rgb(230, 200, 140);
    let warn_bg = egui::Color32::from_rgb(90, 70, 25);

    let mut left_highlights = attribute_highlights(tagged_html, &font, attr_color);
    let mut right_highlights = attribute_highlights(translated_html, &font, attr_color);

    let tagged_units = extract_translation_units(tagged_html);
    let translated_units = extract_translation_units(translated_html);

    let mut left_cursor = 0usize;
    let mut right_cursor = 0usize;
    for (i, tagged_unit) in tagged_units.iter().enumerate() {
        // Lệch số lượng đơn vị (không nên xảy ra vì apply.rs giữ nguyên cấu
        // trúc cây) -> dừng an toàn, không panic, không đoán bừa phần còn lại.
        let Some(translated_unit) = translated_units.get(i) else {
            break;
        };

        if let Some(range) = find_sequential(tagged_html, &tagged_unit.text, &mut left_cursor) {
            left_highlights.push(Highlight {
                range,
                format: text_format_bg(&font, source_color, egui::Color32::TRANSPARENT),
            });
        }

        let is_translated = tagged_unit.text != translated_unit.text;
        if let Some(range) = find_sequential(translated_html, &translated_unit.text, &mut right_cursor) {
            let format = if is_translated {
                text_format_bg(&font, translated_color, egui::Color32::TRANSPARENT)
            } else {
                text_format_bg(&font, warn_text, warn_bg)
            };
            right_highlights.push(Highlight { range, format });
        }
    }

    let mut left = build_job_with_highlights(tagged_html, left_highlights, &font, default_color);
    let mut right = build_job_with_highlights(translated_html, right_highlights, &font, default_color);
    left.wrap.max_width = f32::INFINITY;
    right.wrap.max_width = f32::INFINITY;
    (left, right)
}

/// 1 đơn vị nội dung có thể dịch (text node trực tiếp hoặc giá trị 1 trong 5
/// thuộc tính TRANSLATABLE_ATTRS) — chỉ giữ text, không cần biết thuộc thẻ
/// nào vì việc khớp cặp tagged<->dịch dựa trên CHỈ SỐ trong danh sách (xem
/// doc comment `build_translation_highlight_jobs`).
struct TranslationUnit {
    text: String,
}

/// Duyệt cây ĐÚNG THEO THỨ TỰ apply.rs::apply_translation: với mỗi thẻ đã
/// tag, kiểm tra 5 thuộc tính (theo đúng thứ tự TRANSLATABLE_ATTRS) rồi tới
/// các con text trực tiếp (theo thứ tự xuất hiện) — bỏ qua thuộc
/// tính/text rỗng sau khi trim, khớp đúng quy tắc apply.rs.
fn extract_translation_units(html: &str) -> Vec<TranslationUnit> {
    let document = Html::parse_document(html);
    let mut units = Vec::new();
    for el in document.select(&TAGGED_SELECTOR) {
        let elem = el.value();
        for attr_name in TRANSLATABLE_ATTRS {
            if let Some(value) = elem.attr(attr_name) {
                if !value.trim().is_empty() {
                    units.push(TranslationUnit { text: value.to_string() });
                }
            }
        }
        for child in el.children() {
            if let Node::Text(text) = child.value() {
                if !text.text.trim().is_empty() {
                    units.push(TranslationUnit {
                        text: text.text.to_string(),
                    });
                }
            }
        }
    }
    units
}

/// 1 khoảng byte trong chuỗi HTML đã serialize cần tô theo `format` riêng.
struct Highlight {
    range: Range<usize>,
    format: egui::text::TextFormat,
}

/// Tìm mọi occurrence của `attr_name="..."` (gồm cả tên thuộc tính lẫn giá
/// trị, tới hết dấu ngoặc kép đóng) để tô xám — dùng cho data-builder-id/
/// data-editable/data-editable-attrs, không cần biết trước GIÁ TRỊ cụ thể vì
/// đây là tìm theo TÊN thuộc tính (cú pháp cố định `name="..."`), khác với
/// `find_sequential` (tìm theo NỘI DUNG cụ thể của 1 đơn vị dịch).
fn attribute_highlights(html: &str, font: &egui::FontId, color: egui::Color32) -> Vec<Highlight> {
    let mut highlights = Vec::new();
    for attr_name in ["data-builder-id", "data-editable-attrs", "data-editable"] {
        for range in find_attr_spans(html, attr_name) {
            highlights.push(Highlight {
                range,
                format: text_format(font, color),
            });
        }
    }
    highlights
}

fn find_attr_spans(html: &str, attr_name: &str) -> Vec<Range<usize>> {
    let prefix = format!("{attr_name}=\"");
    let mut spans = Vec::new();
    let mut search_from = 0usize;
    while let Some(rel_start) = html.get(search_from..).and_then(|s| s.find(prefix.as_str())) {
        let start = search_from + rel_start;
        let value_start = start + prefix.len();
        let Some(rel_end) = html.get(value_start..).and_then(|s| s.find('"')) else {
            break; // Không tìm thấy dấu đóng -> dừng, tránh vòng lặp vô hạn.
        };
        let end = value_start + rel_end + 1; // +1 để bao luôn dấu " đóng.
        spans.push(start..end);
        search_from = end;
    }
    spans
}

/// Escape TỐI THIỂU khớp quy tắc serialize text-node/attribute-value của
/// html5ever: `&` và `<` luôn được escape trong text node; `"` cần thiết
/// trong attribute value. Escape cả 3 ký tự ở đây vô hại ngay cả khi 1 ngữ
/// cảnh nào đó html5ever không escape — chuỗi cần tìm khi đó chỉ đơn giản
/// KHÔNG khớp, tự rơi vào nhánh "bỏ qua tô màu" ở `find_sequential`.
fn html_escape_for_search(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;")
}

/// Tìm `needle` (đã escape) trong `haystack`, CHỈ tìm từ `*cursor` trở đi rồi
/// cập nhật `*cursor` tới cuối chỗ vừa tìm thấy — đảm bảo các lần gọi liên
/// tiếp (theo đúng thứ tự duyệt cây) luôn tiến TỚI, không bao giờ tìm lùi
/// lại chỗ đã qua hay tìm trúng 1 occurrence trùng lặp ở nơi khác của cùng
/// nội dung. Trả None (bỏ qua, không panic) nếu không tìm thấy.
fn find_sequential(haystack: &str, needle: &str, cursor: &mut usize) -> Option<Range<usize>> {
    if needle.trim().is_empty() {
        return None;
    }
    let escaped = html_escape_for_search(needle);
    let rel = haystack.get(*cursor..)?.find(escaped.as_str())?;
    let start = *cursor + rel;
    let end = start + escaped.len();
    *cursor = end;
    Some(start..end)
}

/// Ghi `text` vào 1 LayoutJob, áp `highlights` lên đúng những khoảng byte
/// tương ứng (sắp lại theo vị trí trước khi ghi), phần còn lại giữ
/// `default_color`. Bỏ qua (không panic) bất kỳ highlight nào chồng lấn lên
/// phần đã ghi hoặc vượt quá độ dài `text` — về lý thuyết không xảy ra
/// (thuộc tính nằm trong thẻ mở, text node nằm sau thẻ mở, không thể chồng
/// nhau) nhưng an toàn vẫn hơn.
fn build_job_with_highlights(
    text: &str,
    mut highlights: Vec<Highlight>,
    font: &egui::FontId,
    default_color: egui::Color32,
) -> egui::text::LayoutJob {
    highlights.sort_by_key(|h| h.range.start);

    let mut job = egui::text::LayoutJob::default();
    let mut cursor = 0usize;
    for h in highlights {
        if h.range.start < cursor || h.range.end > text.len() || h.range.start > h.range.end {
            continue;
        }
        if h.range.start > cursor {
            job.append(&text[cursor..h.range.start], 0.0, text_format(font, default_color));
        }
        job.append(&text[h.range.start..h.range.end], 0.0, h.format);
        cursor = h.range.end;
    }
    if cursor < text.len() {
        job.append(&text[cursor..], 0.0, text_format(font, default_color));
    }
    job
}

fn text_format_bg(font: &egui::FontId, color: egui::Color32, background: egui::Color32) -> egui::text::TextFormat {
    egui::text::TextFormat {
        font_id: font.clone(),
        color,
        background,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Đếm số dòng trong text thô của 1 LayoutJob (job.text) — cách đơn giản
    /// nhất để kiểm tra "2 cột có cùng số dòng hay không" mà không cần đụng
    /// tới bộ máy layout/font thật của egui trong môi trường test.
    fn line_count(job: &egui::text::LayoutJob) -> usize {
        job.text.lines().count()
    }

    #[test]
    fn replace_block_with_fewer_new_lines_pads_the_right_side() {
        let old = "A\nB\nC\nD\nX\n";
        let new = "A\nZ\nX\n";
        let (left, right) = build_diff_jobs(old, new);
        assert_eq!(line_count(&left), line_count(&right));
    }

    #[test]
    fn replace_block_with_more_new_lines_pads_the_left_side() {
        let old = "A\nB\nX\n";
        let new = "A\nZ1\nZ2\nZ3\nX\n";
        let (left, right) = build_diff_jobs(old, new);
        assert_eq!(line_count(&left), line_count(&right));
    }

    #[test]
    fn pure_delete_then_pure_insert_both_align() {
        let old = "A\nB\nC\nX\n";
        let new = "A\nX\nY1\nY2\n";
        let (left, right) = build_diff_jobs(old, new);
        assert_eq!(line_count(&left), line_count(&right));
    }

    #[test]
    fn identical_input_has_no_padding() {
        let text = "A\nB\nC\n";
        let (left, right) = build_diff_jobs(text, text);
        assert_eq!(line_count(&left), line_count(&right));
        assert_eq!(line_count(&left), 3);
    }

    #[test]
    fn plain_variant_pads_blank_lines_when_serializer_merges_lines() {
        // Ca thực tế người dùng gặp: <!DOCTYPE html>, <html lang="en">,
        // <head> vốn 3 dòng riêng ở bản gốc, nhưng html5ever gộp thành 1
        // dòng khi serialize lại bản đã tag. Nhờ mốc neo <title> khớp y hệt
        // nhau ngay sau đó, thuật toán nhận ra đây là 1 khối "3 dòng cũ -> 1
        // dòng mới" và phải bù đúng 2 dòng TRỐNG vào cột phải để <title>
        // không bị đẩy lệch lên 2 hàng so với cột trái.
        let old = "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<title>x</title>\n";
        let new = "<!DOCTYPE html><html lang=\"en\"><head>\n<title>x</title>\n";
        let (left, right) = build_aligned_plain_jobs(old, new);

        assert_eq!(line_count(&left), line_count(&right));
        // Kiểm tra CỤ THỂ cấu trúc (không chỉ tổng số dòng tình cờ khớp):
        // dòng gộp, rồi 2 dòng trống, rồi mới tới <title> — đúng thứ tự.
        let right_lines: Vec<&str> = right.text.lines().collect();
        assert_eq!(right_lines[0], "<!DOCTYPE html><html lang=\"en\"><head>");
        assert_eq!(right_lines[1], "");
        assert_eq!(right_lines[2], "");
        assert_eq!(right_lines[3], "<title>x</title>");
    }

    #[test]
    fn wrap_is_disabled_so_long_lines_never_auto_wrap() {
        let (left, right) = build_diff_jobs("a\n", "a\n");
        assert_eq!(left.wrap.max_width, f32::INFINITY);
        assert_eq!(right.wrap.max_width, f32::INFINITY);
    }

    // ---- build_translation_highlight_jobs và các hàm phụ trợ ----

    #[test]
    fn extracts_direct_text_child_and_translatable_attrs_in_order() {
        let html = r#"<html><body>
            <input data-builder-id="a_1" data-editable="placeholder" placeholder="Nhap ten">
            <p data-builder-id="a_2" data-editable="text">Xin <b>chao</b> ban</p>
        </body></html>"#;
        let units = extract_translation_units(html);
        // Thứ tự PHẢI đúng thứ tự duyệt cây của apply.rs: thuộc tính của thẻ
        // input trước (vì input đứng trước trong tài liệu), rồi 2 mẩu text
        // TRỰC TIẾP của <p> ("Xin ", " ban") — "chao" nằm trong <b> (chưa tự
        // có data-editable riêng) nên KHÔNG được tính là 1 đơn vị ở đây.
        let texts: Vec<&str> = units.iter().map(|u| u.text.as_str()).collect();
        assert_eq!(texts, vec!["Nhap ten", "Xin ", " ban"]);
    }

    #[test]
    fn find_attr_spans_locates_name_and_value_including_quotes() {
        let html = r#"<p data-builder-id="hero_1" data-editable="text">hi</p>"#;
        let spans = find_attr_spans(html, "data-builder-id");
        assert_eq!(spans.len(), 1);
        assert_eq!(&html[spans[0].clone()], r#"data-builder-id="hero_1""#);
    }

    #[test]
    fn find_sequential_advances_cursor_so_duplicate_text_is_not_matched_twice() {
        let haystack = "one two one three";
        let mut cursor = 0usize;
        let first = find_sequential(haystack, "one", &mut cursor).unwrap();
        assert_eq!(&haystack[first], "one");
        assert_eq!(first.start, 0);
        // Lần tìm THỨ 2 phải nhảy tới occurrence "one" SAU, không tìm lại
        // đúng chỗ cũ, nhờ cursor đã được cập nhật tiến lên.
        let second = find_sequential(haystack, "one", &mut cursor).unwrap();
        assert_eq!(&haystack[second.clone()], "one");
        assert!(second.start > first.start);
    }

    #[test]
    fn highlight_jobs_reconstruct_the_exact_original_text_on_both_sides() {
        let tagged = r#"<p data-builder-id="x_1" data-editable="text">Hello world</p>"#;
        let translated = r#"<p data-builder-id="x_1" data-editable="text">Xin chao</p>"#;
        let (left, right) = build_translation_highlight_jobs(tagged, translated);
        // Bất biến quan trọng nhất: dù chia thành bao nhiêu section màu khác
        // nhau, ghép lại vẫn phải ĐÚNG TUYỆT ĐỐI chuỗi HTML gốc — không mất,
        // không lặp, không xáo trộn ký tự nào.
        assert_eq!(left.text, tagged);
        assert_eq!(right.text, translated);
    }

    #[test]
    fn highlight_jobs_reconstruct_exactly_even_when_untranslated() {
        // Ca "chưa dịch": nội dung y hệt nhau ở cả 2 bên (đường tô nền cảnh
        // báo màu vàng/cam) — vẫn phải ghép lại đúng nguyên văn.
        let html = r#"<p data-builder-id="x_1" data-editable="text">Same text</p>"#;
        let (left, right) = build_translation_highlight_jobs(html, html);
        assert_eq!(left.text, html);
        assert_eq!(right.text, html);
    }

    #[test]
    fn highlight_jobs_do_not_panic_on_plain_non_html_fallback_text() {
        // Ca thực tế: chưa có bản dịch sẵn sàng, preview.rs truyền vào 1
        // chuỗi thông báo thuần tuý (không phải HTML) làm vế phải.
        let tagged = r#"<p data-builder-id="x_1" data-editable="text">Hello</p>"#;
        let (left, right) = build_translation_highlight_jobs(tagged, "(chưa có bản dịch)");
        assert_eq!(left.text, tagged);
        assert_eq!(right.text, "(chưa có bản dịch)");
    }
}
