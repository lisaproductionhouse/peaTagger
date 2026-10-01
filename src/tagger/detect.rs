use ego_tree::NodeId;
use scraper::{ElementRef, Html, Node, Selector};
use std::collections::HashSet;
use std::sync::LazyLock;

/// Selector universal — lọc thật sự nằm trong `classify()`, theo đúng cách
/// auto_tagger.py duyệt nhiều loại thẻ khác nhau bằng nhiều quy tắc riêng
/// biệt (không phải 1 danh sách tag cố định).
static ALL_ELEMENTS: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("*").expect("universal selector luôn hợp lệ"));

/// Chọn đúng các phần tử ĐÃ TỪNG được tag (có CẢ 2 attribute) trong file CŨ —
/// dùng cho `extract_old_candidates`, khớp bs4
/// `find_all(attrs={'data-builder-id': True, 'data-editable': True})` của
/// auto_tagger.py.
static TAGGED_ELEMENTS: LazyLock<Selector> = LazyLock::new(|| {
    Selector::parse("[data-builder-id][data-editable]").expect("selector tĩnh hợp lệ")
});

/// Tiền tố class được auto_tagger.py công nhận là icon font (fa-, bi-,
/// lni-, icon-, icon_, arrow_, social_, ti-) — so khớp theo TỪNG TOKEN của
/// class, token phải BẮT ĐẦU bằng 1 trong các tiền tố này.
const ICON_CLASS_PREFIXES: &[&str] = &["fa-", "bi-", "lni-", "icon-", "icon_", "arrow_", "social_", "ti-"];

/// Danh sách tag được auto_tagger.py coi là "có thể chứa text biên tập
/// được", dùng cho nhánh fallback "text" (Ưu tiên thấp nhất).
const TEXT_TAGS: &[&str] = &[
    "h1", "h2", "h3", "h4", "h5", "h6", "p", "span", "li", "strong", "em", "small", "label",
    "blockquote", "div", "address", "figcaption", "td", "th", "caption", "sup", "sub", "b", "i",
    "q", "cite", "code", "pre", "dt", "dd", "time", "mark", "abbr",
];

static PHONE_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^[+]?[\d\s\-().]{7,20}$").expect("regex tĩnh hợp lệ"));
static EMAIL_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"^[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}$").expect("regex tĩnh hợp lệ")
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ElementType {
    SeoTitle,
    SeoMetaDescription,
    SeoMetaKeywords,
    Counter,
    BgImage,
    OpacityMask,
    Icon,
    Logo,
    LogoMono,
    MainMenuContainer,
    MainMenuItem,
    SubMenuContainer,
    SubMenuItem,
    Link,
    Input,
    Button,
    Image,
    Map,
    Media,
    Text,
}

impl ElementType {
    pub fn as_str(self) -> &'static str {
        match self {
            ElementType::SeoTitle => "seo-title",
            ElementType::SeoMetaDescription => "seo-meta-description",
            ElementType::SeoMetaKeywords => "seo-meta-keywords",
            ElementType::Counter => "counter",
            ElementType::BgImage => "bg-image",
            ElementType::OpacityMask => "opacity-mask",
            ElementType::Icon => "icon",
            ElementType::Logo => "logo",
            ElementType::LogoMono => "logo_mono",
            ElementType::MainMenuContainer => "main-menu-container",
            ElementType::MainMenuItem => "main-menu-item",
            ElementType::SubMenuContainer => "sub-menu-container",
            ElementType::SubMenuItem => "sub-menu-item",
            ElementType::Link => "link",
            ElementType::Input => "input",
            ElementType::Button => "button",
            ElementType::Image => "image",
            ElementType::Map => "map",
            ElementType::Media => "media",
            ElementType::Text => "text",
        }
    }
}

/// 1 phần tử đã nhận dạng, kèm dữ liệu cần để so khớp old<->new (matching.rs)
/// và để tagger::tag_html biết cần bơm thêm gì.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub node_id: NodeId,
    pub element_type: ElementType,
    pub section: String,
    /// Chữ ký dùng để so khớp OLD<->NEW trong matching.rs — xem
    /// `compute_signature`. Thay cho `identity_key` cũ (chỉ dựa vào 1
    /// attribute theo element_type): auto_tagger.py không so khớp theo
    /// "identity key" kiểu đó, mà so theo `get_sig()` — công thức phụ thuộc
    /// THẺ HTML GỐC (meta/img/a/title/khác), rồi dùng `difflib.SequenceMatcher`
    /// trên dãy sig của TỪNG element_type để tìm mốc neo + ghép phần còn lại
    /// theo vị trí. Dùng identity_key kiểu cũ (vd chỉ href cho link, bỏ qua
    /// text) từng khiến kết quả lệch so với bản gốc trong nhiều trường hợp.
    pub sig: String,
    pub text: String,
    pub existing_builder_id: Option<String>,
    /// Danh sách attribute (placeholder/alt/title/content) THỰC SỰ có mặt
    /// trên thẻ — sinh data-editable-attrs="a,b,c" (nhiều giá trị, không
    /// loại trừ nhau, đúng theo auto_tagger.py: 1 thẻ <a title="..."> có
    /// text con VẪN có thể vừa cần dịch text vừa cần dịch title).
    pub editable_attrs: Vec<&'static str>,
    /// data-global-type — đánh dấu ngữ nghĩa đặc biệt (phone/email/address/
    /// map), tách riêng khỏi data-editable vì không đổi taxonomy chính.
    pub global_type: Option<&'static str>,
    /// true nếu là <a> không có text/nội dung hiển thị nào (chỉ bọc icon/
    /// ảnh) VÀ chưa có sẵn style — cần chèn style chống co về 0px.
    pub needs_min_size_style: bool,
}

pub fn detect_candidates(document: &Html) -> Vec<Candidate> {
    // Xử lý TUẦN TỰ theo đúng thứ tự document.select() trả về (thứ tự cây,
    // tổ tiên luôn đứng trước hậu duệ) — cần thiết vì nhánh "text" fallback
    // phải biết 1 tổ tiên của nó có VỪA được xếp là "text" hay chưa trong
    // CHÍNH lượt duyệt này (giống global text_block_nodes của bản gốc), để
    // tránh 1 khối text lớn bị tag lặp lại ở từng thẻ con lồng bên trong.
    let mut candidates = Vec::new();
    let mut text_block_nodes: HashSet<NodeId> = HashSet::new();

    for el in document.select(&ALL_ELEMENTS) {
        if let Some(candidate) = build_candidate(el, &text_block_nodes) {
            if candidate.element_type == ElementType::Text {
                text_block_nodes.insert(candidate.node_id);
            }
            candidates.push(candidate);
        }
    }

    candidates
}

/// 1 phần tử ĐÃ TỪNG được tag trong file CŨ, đọc THẲNG từ 2 attribute đã lưu
/// sẵn trên đó (data-builder-id + data-editable) — KHÔNG chạy lại
/// `classify()` trên nội dung cũ.
///
/// QUAN TRỌNG — đã đối chiếu trực tiếp với auto_tagger.py: bản gốc lấy danh
/// sách phần tử OLD bằng
/// `old_soup.find_all(attrs={'data-builder-id': True, 'data-editable': True})`,
/// tức đọc THẲNG 2 giá trị đã lưu trong file, KHÔNG bao giờ phân loại lại nội
/// dung cũ theo bộ luật `classify()` hiện tại. Trước đây module này gọi
/// `detect_candidates` (chạy full `classify()`) trên CẢ file cũ — sai khác ở
/// 2 điểm: (a) nếu bộ luật classify() thay đổi theo thời gian, phần tử cũ có
/// thể bị phân loại lại khác với lúc nó được gắn, làm lệch nhóm so khớp theo
/// type; (b) mọi phần tử "có thể tag được" trong file cũ (kể cả phần tử CHƯA
/// TỪNG có data-builder-id, do khác batch/khác version tool) đều bị tính vào
/// danh sách OLD, chiếm 1 vị trí trong dãy dùng để so khớp theo VỊ TRÍ dù
/// không có id nào để kế thừa — làm lệch việc ghép cặp cho ĐÚNG các phần tử
/// khác quanh nó. Đọc thẳng attribute như dưới đây tránh cả 2 vấn đề, đúng
/// hệt hành vi bản gốc.
#[derive(Debug, Clone)]
pub struct OldCandidate {
    /// Giá trị `data-editable` đã lưu — dùng làm khoá nhóm khi so khớp (xem
    /// `matching::assign_ids`) THAY VÌ `ElementType`, vì đây là chuỗi đọc
    /// THẲNG từ file, không phải kết quả phân loại lại.
    pub tag_type: String,
    /// Giá trị `data-builder-id` đã lưu — LUÔN có mặt (selector đã lọc sẵn
    /// 2 attribute cùng lúc).
    pub existing_builder_id: String,
    /// Chữ ký dùng để so khớp — công thức giống hệt candidate bên file MỚI,
    /// xem `compute_signature`.
    pub sig: String,
}

/// 4 hậu tố mà `state::Language::id_suffix()` chèn vào cuối data-builder-id
/// lúc export. Liệt kê lại Ở ĐÂY (không import state::Language) để tránh
/// tagger phụ thuộc NGƯỢC lên state (state đã phụ thuộc lên tagger) — đánh
/// đổi: trùng lặp thủ công với id_suffix(), chấp nhận được vì đây là enum
/// đóng 4 giá trị, thêm ngôn ngữ mới vốn dĩ đã phải sửa nhiều nơi ở state.rs.
pub(super) const KNOWN_LANG_ID_SUFFIXES: &[&str] = &["_en", "_vi", "_zh", "_ja"];

/// Bóc hậu tố ngôn ngữ khỏi 1 data-builder-id — LẶP tới khi hết (không chỉ 1
/// lần) để tự phục hồi cả id đã lỡ bị cộng dồn TỪ TRƯỚC khi có fix này (vd
/// "..._en_en" -> "...").
///
/// `pub(super)`: dùng ở CẢ 2 nơi id có thể mang sẵn hậu tố "ngoài ý muốn":
/// (1) `extract_old_candidates` ngay dưới đây — khi người dùng thả thẳng 1
/// file ĐÃ EXPORT (vd "index_en.html", id đã có sẵn "..._en") làm Old; (2)
/// `tagger::append_id_suffix` (mod.rs, module cha) — khi người dùng thả 1
/// file ĐÃ TAG/ĐÃ EXPORT làm NEW (không qua cơ chế Old) — `tag_html` dùng
/// `add_attrs_if_missing` nên GIỮ NGUYÊN id cũ đó, rồi append_id_suffix lại
/// cộng thêm hậu tố NGÔN NGỮ ĐANG XUẤT lên trên, ra "..._en_en". Không bóc ở
/// (1) hoặc (2) thì: (a) id phình dần mỗi vòng Update/export ("..._en_en"),
/// hoặc sai hẳn nếu khác ngôn ngữ ("..._en_vi"); (b) TRAILING_NUMBER trong
/// matching.rs (^(.*)_(\d+)$) không nhận ra số cuối vì id không còn kết thúc
/// bằng số, seed_counter bỏ qua im lặng.
pub(super) fn strip_known_lang_suffix(id: &str) -> &str {
    let mut base = id;
    while let Some(next) = KNOWN_LANG_ID_SUFFIXES.iter().find_map(|s| base.strip_suffix(s)) {
        base = next;
    }
    base
}

pub fn extract_old_candidates(document: &Html) -> Vec<OldCandidate> {
    document
        .select(&TAGGED_ELEMENTS)
        .filter_map(|el| {
            let elem = el.value();
            let tag_type = elem.attr("data-editable")?.to_string();
            let existing_builder_id =
                strip_known_lang_suffix(elem.attr("data-builder-id")?).to_string();
            let full_text = el.text().collect::<String>().trim().to_string();
            let sig = compute_signature(
                elem.name(),
                elem.attr("class").unwrap_or(""),
                elem.attr("name").unwrap_or(""),
                elem.attr("content").unwrap_or(""),
                elem.attr("href").unwrap_or(""),
                elem.attr("src").unwrap_or(""),
                &full_text,
            );
            Some(OldCandidate {
                tag_type,
                existing_builder_id,
                sig,
            })
        })
        .collect()
}

fn build_candidate(el: ElementRef, text_block_nodes: &HashSet<NodeId>) -> Option<Candidate> {
    let element_type = classify(el, text_block_nodes)?;
    let elem = el.value();
    let tag = elem.name();
    let full_text = el.text().collect::<String>().trim().to_string();
    let is_empty_of_text = full_text.is_empty();

    let sig = compute_signature(
        tag,
        elem.attr("class").unwrap_or(""),
        elem.attr("name").unwrap_or(""),
        elem.attr("content").unwrap_or(""),
        elem.attr("href").unwrap_or(""),
        elem.attr("src").unwrap_or(""),
        &full_text,
    );

    // Đúng theo auto_tagger.py: kiểm tra ĐỘC LẬP placeholder/alt/title (mọi
    // loại, không phân biệt element_type) + content (chỉ riêng meta) — có
    // thể có NHIỀU hơn 1 cùng lúc.
    let mut editable_attrs = Vec::new();
    if elem.attr("placeholder").is_some() {
        editable_attrs.push("placeholder");
    }
    if elem.attr("alt").is_some() {
        editable_attrs.push("alt");
    }
    if elem.attr("title").is_some() {
        editable_attrs.push("title");
    }
    if tag == "meta" && elem.attr("content").is_some() {
        editable_attrs.push("content");
    }

    let global_type = detect_global_type(el, element_type, &full_text);

    let needs_min_size_style =
        element_type == ElementType::Link && is_empty_of_text && elem.attr("style").is_none();

    Some(Candidate {
        node_id: el.id(),
        element_type,
        section: find_section(&el),
        sig,
        text: full_text,
        existing_builder_id: elem.attr("data-builder-id").map(str::to_string),
        editable_attrs,
        global_type,
        needs_min_size_style,
    })
}

/// Chữ ký (sig) dùng để so khớp phần tử OLD<->NEW cùng element_type trong
/// matching.rs — mô phỏng CHÍNH XÁC `auto_tagger.py::get_sig` (đã đối chiếu
/// trực tiếp với hàm gốc, không chỉ suy luận): công thức phụ thuộc THẺ HTML
/// GỐC (`el.name`), KHÔNG phụ thuộc element_type/data-editable — vì cùng 1
/// element_type (vd "text") có thể xuất phát từ nhiều thẻ HTML khác nhau
/// (h1, p, span, li...), và auto_tagger.py rẽ nhánh theo tag_name chứ không
/// theo type đã phân loại.
///
/// - meta: name + content (nối thẳng, không dấu phân cách).
/// - img: src.
/// - a: href + 30 KÝ TỰ ĐẦU của text (bao gồm cả text để 1 link đổi hẳn nội
///   dung hiển thị nhưng giữ nguyên href KHÔNG được coi là "y hệt" ở tầng so
///   khớp trực tiếp — vẫn có thể kế thừa id qua cơ chế ghép-theo-vị-trí trong
///   1 "replace block" của LCS, xem matching::assign_gap).
/// - title: toàn bộ text, không cắt.
/// - còn lại (mặc định): "{tag}::{classes}::{50 ký tự đầu của text}" — class
///   được chuẩn hoá khoảng trắng (nhiều khoảng trắng liên tiếp -> 1, giống
///   `" ".join(tag.get('class', []))` của bs4) nhưng GIỮ NGUYÊN hoa/thường.
///
/// Cắt text theo SỐ KÝ TỰ (`.chars()`, không phải byte) để khớp cách Python
/// slice chuỗi Unicode theo code point — quan trọng vì nội dung tiếng Việt/
/// Trung/Nhật dùng ký tự nhiều byte, cắt theo byte có thể cắt giữa 1 ký tự.
fn compute_signature(
    tag: &str,
    class_attr: &str,
    name_attr: &str,
    content_attr: &str,
    href_attr: &str,
    src_attr: &str,
    full_text: &str,
) -> String {
    match tag {
        "meta" => format!("{name_attr}{content_attr}"),
        "img" => src_attr.to_string(),
        "a" => {
            let text_prefix: String = full_text.chars().take(30).collect();
            format!("{href_attr}{text_prefix}")
        }
        "title" => full_text.to_string(),
        _ => {
            let classes = class_attr.split_whitespace().collect::<Vec<_>>().join(" ");
            let text_prefix: String = full_text.chars().take(50).collect();
            format!("{tag}::{classes}::{text_prefix}")
        }
    }
}

/// data-global-type: phone/email (link href hoặc chính text khớp pattern),
/// map (iframe Google Maps), address (khối text nhận diện là địa chỉ) — cả
/// 3 đều là nhãn phụ thêm vào 1 phần tử ĐÃ được xếp loại type chính (link/
/// map/text) rồi.
///
/// SỬA LỖI: comment cũ ở đây ghi "address được quyết định NGAY trong
/// classify()" — ĐÚNG một nửa: Ưu tiên 8 của classify() có quyết định 1
/// khối address thì trả `ElementType::Text` (khớp `add_to_queue(tag, "text",
/// ...)` của auto_tagger.py — address dùng CHUNG `data-editable="text"` với
/// text thường, chỉ khác nhờ data-global-type), nhưng KHÔNG có bước nào gắn
/// lại nhãn "address" vào data-global-type — hàm này trước đây chỉ xử lý
/// Link/Map, bỏ sót nhánh Text nên `data-global-type="address"` không bao
/// giờ xuất hiện trong output (đã đối chiếu trực tiếp với file thật, phát
/// hiện qua so sánh index_en_python.html vs index_en_rust.html: id/type
/// khớp tuyệt đối, chỉ thiếu đúng attribute này).
///
/// Re-check lại ĐÚNG cùng điều kiện gate của Ưu tiên 8
/// (`matches!(tag, "p"|"address"|"div"|"span"|"li") && is_address_block`) là
/// AN TOÀN dù có vẻ trùng lặp tính toán: Ưu tiên 8 trong classify() LUÔN
/// return dứt khoát (Some hoặc None) ngay khi điều kiện gate này đúng, không
/// bao giờ "rơi tiếp" xuống Ưu tiên 11 để bị lẫn với text thường — nên hễ
/// element_type ra tới đây là Text VÀ điều kiện gate này đúng, chắc chắn nó
/// đến từ đúng nhánh address, không thể đến từ nguồn nào khác.
fn detect_global_type(el: ElementRef, element_type: ElementType, full_text: &str) -> Option<&'static str> {
    let elem = el.value();
    match element_type {
        ElementType::Link => {
            let href = elem.attr("href").unwrap_or("");
            if href.starts_with("tel:") || PHONE_RE.is_match(full_text) {
                return Some("phone");
            }
            if href.starts_with("mailto:") || EMAIL_RE.is_match(full_text) {
                return Some("email");
            }
            None
        }
        ElementType::Map => Some("map"),
        ElementType::Text => {
            let tag = elem.name();
            if matches!(tag, "p" | "address" | "div" | "span" | "li") && is_address_block(&el) {
                Some("address")
            } else {
                None
            }
        }
        _ => None,
    }
}

fn classify(el: ElementRef, text_block_nodes: &HashSet<NodeId>) -> Option<ElementType> {
    let elem = el.value();
    let tag = elem.name();
    let class_attr = elem.attr("class").unwrap_or("");
    let class_lower = class_attr.to_lowercase();

    // svg không bao giờ được tag (kết luận từ đối chiếu thực tế) — auto_tagger
    // .py xử lý icon qua <i>/<span>, không bao giờ chạm tới thẻ <svg>.
    if tag == "svg" {
        return None;
    }

    // Ưu tiên 1: SEO.
    if tag == "title" {
        return Some(ElementType::SeoTitle);
    }
    if tag == "meta" {
        // SỬA LỖI: auto_tagger.py hạ chữ thường `name` TRƯỚC khi so
        // ('description'/'keywords') — `name_attr = meta_tag['name'].lower()`.
        // So nguyên văn (case-sensitive) như trước sẽ bỏ sót
        // <meta name="Description" ...> (viết hoa chữ đầu, không hiếm gặp ở
        // 1 số CMS/theme).
        return match elem.attr("name").map(str::to_lowercase).as_deref() {
            Some("description") => Some(ElementType::SeoMetaDescription),
            Some("keywords") => Some(ElementType::SeoMetaKeywords),
            _ => None,
        };
    }

    // Ưu tiên 2: counter — class chứa "counter"/"count-up", HOẶC có attribute
    // bắt đầu bằng data-count/data-purecounter/hoặc chính xác data-to. Loại
    // trừ nếu class chứa "particles" (hay bị nhận nhầm ở hiệu ứng particle
    // background), và loại trừ nếu rỗng text mà lại là div/section (khi đó
    // nhiều khả năng chỉ là container hiệu ứng, không phải số đếm hiển thị).
    if !class_lower.contains("particles") {
        let has_counter_attr = elem.attrs().any(|(name, _)| {
            name.starts_with("data-count") || name.starts_with("data-purecounter") || name == "data-to"
        });
        if class_lower.contains("counter") || class_lower.contains("count-up") || has_counter_attr {
            let has_text = !el.text().collect::<String>().trim().is_empty();
            if has_text || !matches!(tag, "div" | "section") {
                return Some(ElementType::Counter);
            }
        }
    }

    // Ưu tiên 3: bg-image — data-bg/data-background, class chứa
    // bg_image/bg-image/background-image, hoặc inline style background-image.
    if elem.attr("data-bg").is_some() || elem.attr("data-background").is_some() {
        return Some(ElementType::BgImage);
    }
    if class_lower.contains("bg_image") || class_lower.contains("bg-image") || class_lower.contains("background-image") {
        return Some(ElementType::BgImage);
    }
    if let Some(style) = elem.attr("style") {
        if style.contains("background-image") {
            return Some(ElementType::BgImage);
        }
    }

    // Ưu tiên 4: icon — CHỈ <i>/<span>, class có token BẮT ĐẦU bằng 1 trong
    // các tiền tố icon-font đã biết.
    if matches!(tag, "i" | "span") {
        let is_icon = class_attr
            .split_whitespace()
            .any(|token| ICON_CLASS_PREFIXES.iter().any(|p| token.starts_with(p)));
        if is_icon {
            return Some(ElementType::Icon);
        }
    }

    if elem.attr("data-opacity-mask").is_some() {
        return Some(ElementType::OpacityMask);
    }

    // Ưu tiên 5: menu — <ul>/<ol> có class/id chứa "menu", class chứa
    // "navbar-nav", hoặc có tổ tiên <nav> / tổ tiên class chứa "menu". Phân
    // biệt main/sub qua việc có TỔ TIÊN <li> nào không (không chỉ cha trực
    // tiếp — dropdown có thể lồng qua 1 <div> trung gian).
    if matches!(tag, "ul" | "ol") && is_menu_container(&el, &class_lower) {
        return Some(if has_li_ancestor(&el) {
            ElementType::SubMenuContainer
        } else {
            ElementType::MainMenuContainer
        });
    }
    if tag == "li" {
        if let Some(parent) = el.parent() {
            if let Node::Element(parent_elem) = parent.value() {
                if matches!(parent_elem.name(), "ul" | "ol") {
                    let parent_class = parent_elem.attr("class").unwrap_or("").to_lowercase();
                    // Cha (ul/ol) có được coi là menu không? Kiểm tra lại
                    // đúng điều kiện is_menu_container nhưng xuất phát từ cha,
                    // không tạo ElementRef mới nên kiểm tra trực tiếp qua attr.
                    let parent_is_menu = parent_class.contains("menu")
                        || parent_elem.attr("id").unwrap_or("").to_lowercase().contains("menu")
                        || parent_class.contains("navbar-nav")
                        || is_within_nav_or_menu_ancestor(&el);
                    if parent_is_menu {
                        return Some(if has_li_ancestor(&el) {
                            ElementType::SubMenuItem
                        } else {
                            ElementType::MainMenuItem
                        });
                    }
                }
            }
        }
    }

    // Ưu tiên 6: ảnh — logo/logo_mono theo class/src CHÍNH NÓ hoặc TỔ TIÊN
    // chứa "logo"/"brand"; còn lại là "image" thường.
    if tag == "img" {
        let src_lower = elem.attr("src").unwrap_or("").to_lowercase();
        let is_logo_self = class_lower.contains("logo") || src_lower.contains("logo");
        let is_logo_ancestor = ancestor_id_or_class_contains(&el, &["logo", "brand"]);
        if is_logo_self || is_logo_ancestor {
            let is_mono = class_lower.contains("sticky")
                || class_lower.contains("white")
                || class_lower.contains("light")
                || class_lower.contains("mono")
                || src_lower.contains("sticky")
                || src_lower.contains("white")
                || src_lower.contains("mono")
                || ancestor_id_or_class_contains(&el, &["footer", "dark"])
                || is_within_footer(&el);
            return Some(if is_mono { ElementType::LogoMono } else { ElementType::Logo });
        }
        return Some(ElementType::Image);
    }

    // Ưu tiên 7: link.
    if tag == "a" && elem.attr("href").is_some() {
        return Some(ElementType::Link);
    }

    // Ưu tiên 8: khối "address" — p/address/div/span/li khớp 1 trong các
    // dấu hiệu: chính nó là <address>, class (token, so khớp CHÍNH XÁC —
    // không phải substring) chứa "address", heading liền trước chứa chữ
    // "address", hoặc class CHA (substring) chứa "address". Nếu nội dung
    // gần như chỉ là 1 <a> (link chiếm hầu hết độ dài text) thì bỏ qua, để
    // link đó tự đứng riêng thay vì bọc thêm 1 lớp "text" không cần thiết.
    if matches!(tag, "p" | "address" | "div" | "span" | "li") && is_address_block(&el) {
        let (a_len, total_len) = text_wrapped_by_anchor_ratio(&el);
        if total_len == 0 || total_len > a_len {
            return Some(ElementType::Text);
        }
        return None;
    }

    // Ưu tiên 9: iframe — Google Maps thì "map", còn lại (video nhúng...)
    // là "media". <video> cũng thuộc "media".
    if tag == "iframe" {
        let src_lower = elem.attr("src").unwrap_or("").to_lowercase();
        if src_lower.contains("google.com/maps") {
            return Some(ElementType::Map);
        }
        return Some(ElementType::Media);
    }
    if tag == "video" {
        return Some(ElementType::Media);
    }

    // Ưu tiên 10: button — <button>, hoặc <input type="submit|button|reset">
    // có value không rỗng (input dùng LÀM nút bấm, không phải ô nhập liệu).
    if tag == "button" {
        let has_text = !el.text().collect::<String>().trim().is_empty();
        return has_text.then_some(ElementType::Button);
    }
    if tag == "input" {
        let input_type = elem.attr("type").unwrap_or("text").to_lowercase();
        if matches!(input_type.as_str(), "submit" | "button" | "reset") {
            let has_value = elem.attr("value").is_some_and(|v| !v.trim().is_empty());
            return has_value.then_some(ElementType::Button);
        }
    }

    // Ưu tiên 11 (fallback chung, thấp nhất): text. Loại trừ nếu:
    // - thẻ nằm trong danh sách loại trừ kỹ thuật, hoặc bên trong nó chứa
    //   form control/media (input/select/textarea/form/img/video/iframe) —
    //   khi đó nội dung chính là các phần tử con đó, không phải text thuần;
    // - <label for="..."> gắn với 1 checkbox/radio (nhãn đi kèm control,
    //   không phải văn bản độc lập cần dịch riêng);
    // - đã nằm trong 1 khối "text" khác được xử lý trước đó trong CÙNG lượt
    //   duyệt (text_block_nodes) — tránh tag lồng nhau thừa;
    // - có text nhưng gần như toàn bộ nằm trong 1 <a> con (xem Ưu tiên 8,
    //   áp dụng chung logic a_len/total_len);
    // - KHÔNG có text TRỰC TIẾP của riêng nó (chỉ là container bọc thẻ con)
    //   VÀ có ít nhất 1 thẻ con trực tiếp — dành phần đó cho CHÍNH các thẻ
    //   con tự đứng riêng, không gộp cả khối cha thành 1 "text" duy nhất.
    //
    // SỬA LỖI (đã đối chiếu trực tiếp với file thật, xem confirm_all_26_v2.py):
    // điều kiện cũ ở đây là `has_direct_text(&el) || total_len > 0` —
    // `total_len` (từ text_wrapped_by_anchor_ratio) cộng dồn CẢ text nằm lồng
    // sâu bên trong thẻ con, nên gần như LUÔN dương bất cứ khi nào phần tử có
    // text ở đâu đó bên trong — khiến điều kiện này gần như vô nghĩa (gần như
    // luôn true). Hậu quả: 1 <div> chỉ bọc <h2>+<p> (không có text riêng nào
    // là con trực tiếp của chính div) bị tag gộp làm 1 "text" duy nhất (nuốt
    // luôn nội dung h2+p vào 1 id), thay vì auto_tagger.py bỏ qua div này để
    // <h2> và <p> tự đứng ra tag riêng từng cái — đúng theo điều kiện
    // `if not direct_text and len(child_elements) > 0: continue` của bản gốc.
    if TEXT_TAGS.contains(&tag) && !is_nested_in_text_block(&el, text_block_nodes) {
        if has_disqualifying_descendant(&el) {
            return None;
        }
        if tag == "label" {
            if let Some(for_id) = elem.attr("for") {
                if input_type_of_id(&el, for_id).is_some_and(|t| matches!(t.as_str(), "checkbox" | "radio")) {
                    return None;
                }
            }
        }
        let (a_len, total_len) = text_wrapped_by_anchor_ratio(&el);
        if total_len == 0 {
            // Không có text nào cả, kể cả lồng sâu bên trong con — khớp
            // `not tag.get_text(strip=True): continue` (điều kiện ĐẦU TIÊN,
            // độc lập) của auto_tagger.py. Cần tách riêng khỏi check bên dưới:
            // trước đây `total_len > 0` gộp chung trong điều kiện tag-hay-không
            // nên "vô tình" gánh luôn vai trò này; bỏ nó đi (để sửa bug chính)
            // mà không thêm lại điều kiện này riêng sẽ khiến 1 thẻ HOÀN TOÀN
            // RỖNG (vd <em></em> trống, không children, không text) bị tag
            // nhầm thành "text" (vì lúc đó has_direct_child_element cũng false).
            return None;
        }
        if total_len <= a_len {
            return None;
        }
        if has_direct_text(&el) || !has_direct_child_element(&el) {
            return Some(ElementType::Text);
        }
    }

    // Ưu tiên 12: input/textarea kiểu nhập liệu văn bản, CÓ placeholder.
    if matches!(tag, "input" | "textarea") {
        let input_type = elem.attr("type").unwrap_or("text").to_lowercase();
        let is_text_like = tag == "textarea"
            || matches!(input_type.as_str(), "text" | "email" | "search" | "tel" | "url" | "number" | "password" | "");
        if is_text_like && elem.attr("placeholder").is_some() {
            return Some(ElementType::Input);
        }
    }

    None
}

/// true nếu (ul/ol) `el` được coi là 1 menu container — class/id chứa
/// "menu", class chứa "navbar-nav", hoặc có tổ tiên <nav>/tổ tiên class
/// chứa "menu".
fn is_menu_container(el: &ElementRef, class_lower: &str) -> bool {
    let id_lower = el.value().attr("id").unwrap_or("").to_lowercase();
    class_lower.contains("menu")
        || id_lower.contains("menu")
        || class_lower.contains("navbar-nav")
        || is_within_nav_or_menu_ancestor(el)
}

/// true nếu có tổ tiên là <nav>, hoặc tổ tiên nào đó có class chứa "menu".
fn is_within_nav_or_menu_ancestor(el: &ElementRef) -> bool {
    let mut current = el.parent();
    while let Some(node) = current {
        if let Node::Element(e) = node.value() {
            if e.name() == "nav" {
                return true;
            }
            if e.attr("class").unwrap_or("").to_lowercase().contains("menu") {
                return true;
            }
        }
        current = node.parent();
    }
    false
}

/// true nếu `el` có bất kỳ tổ tiên nào là <li> (không chỉ cha trực tiếp).
fn has_li_ancestor(el: &ElementRef) -> bool {
    let mut current = el.parent();
    while let Some(node) = current {
        if matches!(node.value(), Node::Element(e) if e.name() == "li") {
            return true;
        }
        current = node.parent();
    }
    false
}

/// true nếu <footer> là tổ tiên của `el`.
fn is_within_footer(el: &ElementRef) -> bool {
    let mut current = el.parent();
    while let Some(node) = current {
        if matches!(node.value(), Node::Element(e) if e.name() == "footer") {
            return true;
        }
        current = node.parent();
    }
    false
}

/// true nếu BẤT KỲ tổ tiên nào có id hoặc class (substring) chứa 1 trong
/// các `needles` — dùng cho logo (id/class tổ tiên chứa "logo"/"brand").
fn ancestor_id_or_class_contains(el: &ElementRef, needles: &[&str]) -> bool {
    let mut current = el.parent();
    while let Some(node) = current {
        if let Node::Element(e) = node.value() {
            let id_lower = e.attr("id").unwrap_or("").to_lowercase();
            let class_lower = e.attr("class").unwrap_or("").to_lowercase();
            if needles.iter().any(|n| id_lower.contains(n) || class_lower.contains(n)) {
                return true;
            }
        }
        current = node.parent();
    }
    false
}

/// Khối "address": chính nó là <address>, class (TOKEN khớp chính xác, theo
/// đúng auto_tagger.py) chứa "address", heading (h2..h6) liền TRƯỚC chứa
/// chữ "address", hoặc class CHA (substring) chứa "address".
fn is_address_block(el: &ElementRef) -> bool {
    let elem = el.value();
    if elem.name() == "address" {
        return true;
    }
    let has_address_class_token = elem
        .attr("class")
        .unwrap_or("")
        .split_whitespace()
        .any(|t| t.eq_ignore_ascii_case("address"));
    if has_address_class_token {
        return true;
    }
    if let Some(prev) = previous_sibling_element(el) {
        if let Node::Element(e) = prev.value() {
            if matches!(e.name(), "h2" | "h3" | "h4" | "h5" | "h6") {
                let text = collect_node_text(prev);
                if text.to_lowercase().contains("address") {
                    return true;
                }
            }
        }
    }
    if let Some(parent) = el.parent() {
        if let Node::Element(e) = parent.value() {
            if e.attr("class").unwrap_or("").to_lowercase().contains("address") {
                return true;
            }
        }
    }
    false
}

/// Gom toàn bộ text bên trong 1 NodeRef (không cần ElementRef) — dùng cho
/// heading liền trước 1 khối address, tránh phụ thuộc ElementRef::wrap.
fn collect_node_text(node: ego_tree::NodeRef<Node>) -> String {
    let mut result = String::new();
    for descendant in node.descendants() {
        if let Node::Text(t) = descendant.value() {
            result.push_str(&t.text);
        }
    }
    result
}

/// Node anh/em liền TRƯỚC (bỏ qua text node thuần khoảng trắng), trả về
/// dạng NodeRef chung (không cần ElementRef) để không phụ thuộc API rộng.
fn previous_sibling_element<'a>(el: &ElementRef<'a>) -> Option<ego_tree::NodeRef<'a, Node>> {
    let mut current = el.prev_sibling();
    while let Some(node) = current {
        if matches!(node.value(), Node::Element(_)) {
            return Some(node);
        }
        current = node.prev_sibling();
    }
    None
}

/// (độ dài text nằm trong 1 <a> hậu duệ nào đó, tổng độ dài text toàn bộ) —
/// dùng để loại các thẻ mà nội dung gần như chỉ là 1 link, tránh tag "text"
/// thừa lên trên 1 link đã tự đứng riêng.
fn text_wrapped_by_anchor_ratio(el: &ElementRef) -> (usize, usize) {
    let mut a_len = 0usize;
    let mut total_len = 0usize;
    for descendant in el.descendants() {
        if let Node::Text(t) = descendant.value() {
            let len = t.text.trim().chars().count();
            if len == 0 {
                continue;
            }
            total_len += len;
            let mut current = descendant.parent();
            while let Some(node) = current {
                if node.id() == el.id() {
                    break;
                }
                if matches!(node.value(), Node::Element(e) if e.name() == "a") {
                    a_len += len;
                    break;
                }
                current = node.parent();
            }
        }
    }
    (a_len, total_len)
}

/// true nếu `el` chứa (ở bất kỳ độ sâu nào) 1 trong các thẻ khiến nó KHÔNG
/// nên được coi là khối text thuần (form control hoặc media nhúng).
fn has_disqualifying_descendant(el: &ElementRef) -> bool {
    el.descendants().any(|node| {
        matches!(node.value(), Node::Element(e) if matches!(
            e.name(),
            "input" | "select" | "textarea" | "form" | "img" | "video" | "iframe"
        ))
    })
}

/// Tìm type của <input id="for_id"> trong phạm vi CHA của `el` (label) —
/// đủ cho phần lớn thực tế (label + input thường là anh em/nằm gần nhau
/// trong cùng 1 form-group), tránh phải đi tới tận gốc document.
fn input_type_of_id(el: &ElementRef, for_id: &str) -> Option<String> {
    let container = el.parent()?;
    container.descendants().find_map(|node| {
        if let Node::Element(e) = node.value() {
            if e.name() == "input" && e.attr("id") == Some(for_id) {
                return e.attr("type").map(|t| t.to_lowercase());
            }
        }
        None
    })
}

/// true nếu `el` có ÍT NHẤT 1 text node là CON TRỰC TIẾP (không phải cháu/
/// chắt) với nội dung khác rỗng.
fn has_direct_text(el: &ElementRef) -> bool {
    el.children()
        .any(|child| matches!(child.value(), Node::Text(t) if !t.text.trim().is_empty()))
}

/// true nếu `el` có ÍT NHẤT 1 THẺ CON TRỰC TIẾP (không phải text node) —
/// khớp `len(tag.find_all(True, recursive=False)) > 0` của auto_tagger.py.
/// Dùng ở Ưu tiên 11: 1 container CHỈ bọc thẻ con (không có text riêng nào
/// là con trực tiếp của chính nó) phải được BỎ QUA để chính các thẻ con đó
/// tự đứng ra tag riêng, thay vì gộp cả khối cha thành 1 "text" duy nhất.
fn has_direct_child_element(el: &ElementRef) -> bool {
    el.children().any(|child| matches!(child.value(), Node::Element(_)))
}

/// true nếu `el` có tổ tiên nào đó nằm trong `text_block_nodes` (đã được
/// chính lượt duyệt này xếp là "text" trước đó).
fn is_nested_in_text_block(el: &ElementRef, text_block_nodes: &HashSet<NodeId>) -> bool {
    let mut current = el.parent();
    while let Some(node) = current {
        if text_block_nodes.contains(&node.id()) {
            return true;
        }
        current = node.parent();
    }
    false
}

/// Dò ngược lên tổ tiên tìm ELEMENT GẦN NHẤT CÓ THUỘC TÍNH id, dùng làm
/// "[section]". Không có id nào -> fallback landmark: {header, nav} ->
/// "header", {footer} -> "footer" (đúng theo auto_tagger.py — không tách
/// main/aside/section riêng). Không có gì cả -> "page".
///
/// SỬA LẠI cho khớp `auto_tagger.py::get_section_name` + `sanitize_name` (đã
/// đối chiếu trực tiếp, xem verify_sanitize*.py) — đây là nguồn lệch tagid
/// chính so với bản gốc, 3 điểm cụ thể:
///
/// 1. `id.replace('-', "_")` cũ CHỈ đổi gạch ngang — thiếu hạ chữ thường và
///    thiếu thay các ký tự đặc biệt KHÁC (khoảng trắng, dấu chấm...) bằng
///    gạch dưới. `id="Main Content.Area"` phải ra "main_content_area", không
///    phải giữ nguyên "Main Content.Area". Xem `sanitize_name` bên dưới.
/// 2. `find_parent(attrs={"id": True})` của bản gốc dừng lại ở tổ tiên GẦN
///    NHẤT có thuộc tính id — DÙ id đó rỗng — chứ không tiếp tục dò tổ tiên
///    xa hơn để tìm 1 id khác. Bản cũ ở đây làm ngược lại (bỏ qua id rỗng rồi
///    dò tiếp), khiến 1 số trường hợp ra section khác bản gốc.
/// 3. Bản gốc gọi `find_parent(['header','nav'])` TRƯỚC (dò toàn bộ chuỗi tổ
///    tiên, không chỉ tổ tiên gần nhất), chỉ khi không thấy mới gọi tiếp
///    `find_parent('footer')` — nên 1 <header> tổ tiên XA vẫn thắng 1
///    <footer> tổ tiên GẦN hơn. Bản cũ chỉ lấy landmark ĐẦU TIÊN gặp khi dò
///    ngược, có thể cho kết quả ngược lại nếu footer lồng trong header.
fn find_section(el: &ElementRef) -> String {
    let mut current = el.parent();

    while let Some(node) = current {
        if let Node::Element(element) = node.value() {
            if let Some(id) = element.attr("id") {
                // Đúng theo bs4 `find_parent(attrs={"id": True})`: dừng dò ở
                // đây luôn (dù id rỗng), KHÔNG tiếp tục lên tổ tiên xa hơn.
                return if id.is_empty() {
                    landmark_or_page(el)
                } else {
                    sanitize_name(id)
                };
            }
        }
        current = node.parent();
    }

    landmark_or_page(el)
}

/// Mô phỏng 2 lời gọi `find_parent` RIÊNG BIỆT của auto_tagger.py:
/// `find_parent(['header','nav'])` trước (dò TOÀN BỘ chuỗi tổ tiên của `el`,
/// không chỉ tổ tiên gần nhất), rồi CHỈ khi không thấy mới
/// `find_parent('footer')`. Vì vậy 1 <header> tổ tiên xa vẫn thắng 1 <footer>
/// tổ tiên gần hơn — khác hẳn việc chỉ lấy landmark ĐẦU TIÊN gặp khi dò
/// ngược từ `el`.
fn landmark_or_page(el: &ElementRef) -> String {
    if has_ancestor_named(el, &["header", "nav"]) {
        return "header".to_string();
    }
    if has_ancestor_named(el, &["footer"]) {
        return "footer".to_string();
    }
    "page".to_string()
}

/// true nếu `el` có tổ tiên nào đó (ở BẤT KỲ độ sâu nào, không chỉ gần nhất)
/// mang 1 trong các tên thẻ `names`.
fn has_ancestor_named(el: &ElementRef, names: &[&str]) -> bool {
    let mut current = el.parent();
    while let Some(node) = current {
        if matches!(node.value(), Node::Element(e) if names.contains(&e.name())) {
            return true;
        }
        current = node.parent();
    }
    false
}

/// Mô phỏng CHÍNH XÁC `auto_tagger.py::sanitize_name` (đã đối chiếu trực
/// tiếp, xem verify_sanitize.py): hạ chữ thường CHỈ ASCII a-z/0-9 được giữ
/// nguyên (khớp `[a-zA-Z0-9]` của regex gốc — ký tự có dấu tiếng Việt/Trung/
/// Nhật KHÔNG nằm trong tập này nên cũng bị thay bằng gạch dưới, đúng hành vi
/// gốc dù nhìn hơi lạ với nội dung tiếng Việt), mọi ký tự khác (kể cả gạch
/// ngang, khoảng trắng, dấu chấm...) gộp thành 1 dấu gạch dưới, rồi cắt gạch
/// dưới thừa ở 2 đầu chuỗi.
fn sanitize_name(id: &str) -> String {
    let mut result = String::with_capacity(id.len());
    let mut last_was_sep = false;
    for ch in id.chars() {
        if ch.is_ascii_alphanumeric() {
            result.push(ch.to_ascii_lowercase());
            last_was_sep = false;
        } else if !last_was_sep {
            result.push('_');
            last_was_sep = true;
        }
    }
    result.trim_matches('_').to_string()
}
