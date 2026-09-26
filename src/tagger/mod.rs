mod detect;
mod matching;

use html5ever::tree_builder::TreeSink;
use html5ever::{Attribute, LocalName, Namespace, QualName};
use scraper::{Html, HtmlTreeSink};

/// Phân tích `new_html`, gắn `data-builder-id` + `data-editable` vào các thẻ
/// nhận dạng được (xem detect::classify), kế thừa id từ `old_html` (nếu có)
/// theo chữ ký + vị trí — mô phỏng đúng thuật toán
/// `difflib.SequenceMatcher` của auto_tagger.py (xem matching::assign_ids),
/// trả về HTML đã xử lý dưới dạng chuỗi.
///
/// `old_html` được đọc qua `detect::extract_old_candidates` (CHỈ lấy phần tử
/// đã có sẵn data-builder-id + data-editable, đọc THẲNG 2 giá trị đó) chứ
/// KHÔNG qua `detect::detect_candidates` — file cũ không được re-classify lại
/// bằng bộ luật classify() hiện tại, đúng cách auto_tagger.py xử lý file cũ.
///
/// Lưu ý: html5ever sẽ CHUẨN HOÁ HTML một phần khi serialize lại (tự thêm
/// <html>/<head>/<body> nếu thiếu, tự đóng thẻ, chuẩn hoá dấu nháy...) — đây
/// là hành vi bình thường của MỌI thư viện parse->modify->serialize, không
/// phải lỗi của module này.
pub fn tag_html(new_html: &str, old_html: Option<&str>) -> String {
    let old_candidates = old_html
        .map(|html| detect::extract_old_candidates(&Html::parse_document(html)))
        .unwrap_or_default();

    let mut document = Html::parse_document(new_html);
    let new_candidates = detect::detect_candidates(&document);
    let assignments = matching::assign_ids(&new_candidates, &old_candidates);

    let tree = HtmlTreeSink::new(document);
    for candidate in &new_candidates {
        if let Some(builder_id) = assignments.get(&candidate.node_id) {
            let mut attrs = vec![
                make_attr("data-builder-id", builder_id),
                make_attr("data-editable", candidate.element_type.as_str()),
            ];
            if !candidate.editable_attrs.is_empty() {
                // Nhiều attribute có thể CÙNG LÚC cần dịch (vd <a title="...">
                // vừa có text con vừa có title) — nối bằng dấu phẩy, đúng theo
                // bản gốc, không phải chỉ 1 attribute duy nhất.
                attrs.push(make_attr("data-editable-attrs", &candidate.editable_attrs.join(",")));
            }
            if let Some(global_type) = candidate.global_type {
                attrs.push(make_attr("data-global-type", global_type));
            }
            if candidate.needs_min_size_style {
                // Link không có text/nội dung hiển thị (chỉ bọc icon/ảnh) dễ
                // bị co về 0x0px, khó thấy/khó bấm trong Web Builder — chèn
                // kích thước tối thiểu để luôn có vùng bấm/chọn được.
                attrs.push(make_attr(
                    "style",
                    "display:inline-block;min-width:22px;min-height:1em;",
                ));
            }
            // add_attrs_if_missing: nếu thẻ NEW vô tình đã có sẵn các
            // attribute này (vd lỡ thả nhầm file đã tag làm file New) thì sẽ
            // KHÔNG bị ghi đè. Chấp nhận được vì file New theo đúng luồng sử
            // dụng luôn là HTML thô, chưa qua tool này lần nào.
            tree.add_attrs_if_missing(&candidate.node_id, attrs);
        }
    }
    document = tree.finish();
    document.html()
}

/// Nối `suffix` (vd "_en") vào cuối MỌI `data-builder-id` đã có trong `html`.
/// Dùng cho Phần 5 khi checkbox "Thêm hậu tố ngôn ngữ vào ID" bật, để Web
/// Builder phân biệt được id của từng bản ngôn ngữ (vd `main_paragraph_1_en`
/// vs `main_paragraph_1_vi`) dù cùng trỏ tới 1 vị trí trên trang.
///
/// Ghi đè trực tiếp qua `document.tree.get_mut()` (giống translate/apply.rs)
/// thay vì TreeSink::add_attrs_if_missing ở trên — hàm đó CHỦ ĐÍCH không ghi
/// đè, ở đây thì ngược lại, cần ghi đè giá trị cũ bằng giá trị đã nối hậu tố.
pub fn append_id_suffix(html: &str, suffix: &str) -> String {
    let selector = scraper::Selector::parse("[data-builder-id]")
        .expect("selector tĩnh hợp lệ");
    let document = Html::parse_document(html);

    let targets: Vec<(ego_tree::NodeId, String)> = document
        .select(&selector)
        .filter_map(|el| {
            let current = el.value().attr("data-builder-id")?;
            Some((el.id(), format!("{current}{suffix}")))
        })
        .collect();

    let mut document = document;
    // element.attrs là Vec<(QualName, StrTendril)>, không phải kiểu map có
    // .insert(key, value) — dò theo LocalName (tên attribute) rồi gán trực
    // tiếp vào entry tìm được; attribute này CHẮC CHẮN đã tồn tại vì targets
    // chỉ chứa node lấy được từ selector "[data-builder-id]" ở trên. So theo
    // .local thay vì so cả QualName để không phụ thuộc namespace/prefix tự
    // dựng có khớp tuyệt đối với cái html5ever gán lúc parse hay không.
    let target_local = LocalName::from("data-builder-id");
    for (node_id, new_value) in targets {
        let Some(mut node_mut) = document.tree.get_mut(node_id) else {
            continue;
        };
        if let scraper::Node::Element(element) = node_mut.value() {
            for (name, value) in element.attrs.iter_mut() {
                if name.local == target_local {
                    *value = new_value.into();
                    break;
                }
            }
        }
    }

    document.html()
}

fn make_attr(name: &str, value: &str) -> Attribute {
    Attribute {
        name: QualName::new(None, Namespace::from(""), LocalName::from(name)),
        value: value.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_common_elements_with_no_old_html() {
        let html = r#"<html><head><title>Hello</title></head><body>
            <h1>Welcome</h1>
            <p>Some paragraph text.</p>
            <a href="/about">About</a>
        </body></html>"#;

        let output = tag_html(html, None);

        // title -> "seo-title" riêng, không phải "title" chung chung.
        assert!(output.contains(r#"data-editable="seo-title""#));
        // heading/paragraph gộp chung thành "text" (khớp taxonomy reference).
        assert!(output.contains(r#"data-editable="text""#));
        assert!(output.contains(r#"data-editable="link""#));
        assert!(output.contains("data-builder-id="));
        // Không có ancestor mang id, không có landmark HTML5 nào bao ngoài
        // -> section fallback cuối cùng là "page", không phải "body".
        assert!(output.contains("page_text_1"));
    }

    #[test]
    fn inherits_id_by_matching_text_content() {
        let old_html = r#"<html><body>
            <p data-builder-id="page_text_1" data-editable="text">Giu nguyen noi dung nay.</p>
        </body></html>"#;
        let new_html = r#"<html><body>
            <p>Giu nguyen noi dung nay.</p>
        </body></html>"#;

        let output = tag_html(new_html, Some(old_html));

        assert!(output.contains(r#"data-builder-id="page_text_1""#));
    }

    #[test]
    fn inherits_id_via_positional_replace_when_only_href_stays_same() {
        // sig của link = href + text[:30] (xem compute_signature), nên href
        // giữ nguyên nhưng text đổi HẲN vẫn cho ra sig KHÁC NHAU — không khớp
        // ở mốc neo LCS 'equal'. Vẫn kế thừa được id nhờ đây là 1 "replace
        // block" trọn vẹn: cả 2 danh sách (old/new) chỉ có ĐÚNG 1 phần tử, nên
        // được ghép theo vị trí — y hệt opcode 'replace' của
        // difflib.SequenceMatcher trong auto_tagger.py (đã đối chiếu trực
        // tiếp, xem verify_lcs.py).
        let old_html = r#"<html><body>
            <a href="/contact" data-builder-id="page_link_1" data-editable="link">Lien he</a>
        </body></html>"#;
        let new_html = r#"<html><body>
            <a href="/contact">Lien he chung toi</a>
        </body></html>"#;

        let output = tag_html(new_html, Some(old_html));

        assert!(output.contains(r#"data-builder-id="page_link_1""#));
    }

    #[test]
    fn new_elements_get_fresh_sequential_ids_without_colliding() {
        let old_html = r#"<html><body><p data-builder-id="page_text_1" data-editable="text">A</p></body></html>"#;
        let new_html = r#"<html><body>
            <p>A</p>
            <p>Doan hoan toan moi, khong co trong ban cu.</p>
        </body></html>"#;

        let output = tag_html(new_html, Some(old_html));

        assert!(output.contains(r#"data-builder-id="page_text_1""#)); // kế thừa
        assert!(output.contains(r#"data-builder-id="page_text_2""#)); // sinh mới, không trùng
    }

    #[test]
    fn fresh_id_continues_from_highest_number_ever_used_in_old_file() {
        // SỬA LỖI: bộ đếm sinh id mới cho 1 base_name phải khởi tạo từ SỐ LỚN
        // NHẤT từng xuất hiện cho base_name đó trong TOÀN BỘ file CŨ — đúng
        // theo get_unique_id/id_counters của auto_tagger.py — chứ không phải
        // luôn bắt đầu đếm lại từ 1 rồi chỉ né những số đang "bận" trong lượt
        // chạy này. File cũ ở đây từng dùng tới số 7 cho "page_text"; phần tử
        // "Cu roi" khớp lại đúng "page_text_7", còn phần tử hoàn toàn mới
        // ("Hoan toan moi...") không còn phần tử OLD nào để ghép (dù bằng mốc
        // neo LCS hay bằng ghép-vị-trí) nên phải sinh id mới — id đó PHẢI là
        // "page_text_8" (tiếp theo sau 7), TUYỆT ĐỐI không phải "page_text_1".
        let old_html = r#"<html><body>
            <p data-builder-id="page_text_7" data-editable="text">Cu roi</p>
        </body></html>"#;
        let new_html = r#"<html><body>
            <p>Cu roi</p>
            <p>Hoan toan moi, khong lien quan gi noi dung cu.</p>
        </body></html>"#;

        let output = tag_html(new_html, Some(old_html));

        assert!(output.contains(r#"data-builder-id="page_text_7""#));
        assert!(output.contains(r#"data-builder-id="page_text_8""#));
        assert!(!output.contains(r#"data-builder-id="page_text_1""#));
    }

    #[test]
    fn section_uses_nearest_ancestor_id_over_landmark() {
        // <li> nằm trong <ul id="top_menu"> PHẢI lấy section "top_menu" (từ
        // id gần nhất), KHÔNG phải "header" (landmark bao ngoài xa hơn) —
        // đúng quy tắc quan sát được từ file reference.
        let html = r#"<html><body><header>
            <ul id="top_menu"><li>Home</li></ul>
        </header></body></html>"#;

        let output = tag_html(html, None);

        assert!(output.contains("top_menu_main-menu-item_1"));
        assert!(!output.contains("header_main-menu-item"));
    }

    #[test]
    fn section_from_ancestor_id_is_sanitized_like_python_reference() {
        // SỬA LỖI: auto_tagger.py::sanitize_name hạ chữ thường + thay MỌI ký
        // tự không phải chữ/số bằng gạch dưới (không chỉ riêng gạch ngang)
        // trước khi dùng id tổ tiên làm [section]. id="Main Content.Area"
        // phải cho ra section "main_content_area" — bản trước chỉ đổi "-"
        // nên sẽ giữ nguyên hoa/thường và khoảng trắng/dấu chấm, ra 1 section
        // hoàn toàn khác bản Python (đây là nguồn lệch tagid phổ biến nhất,
        // vì id thực tế trên web rất hay có hoa/thường hoặc khoảng trắng).
        let html = r#"<html><body>
            <div id="Main Content.Area">
                <a href="/lien-he">Lien he</a>
            </div>
        </body></html>"#;

        let output = tag_html(html, None);

        assert!(output.contains("main_content_area_link_1"));
    }

    #[test]
    fn menu_requires_menu_signal_not_just_being_inside_header() {
        // Đúng theo auto_tagger.py: <ul>/<ol> được coi là menu khi class/id
        // chứa "menu" HOẶC có tổ tiên <nav>/tổ tiên class chứa "menu" — CHỈ
        // nằm trong <header> KHÔNG đủ nếu không có tín hiệu nào ở trên.
        let html = r#"<html><body>
            <header><ul id="main-menu"><li>Home</li></ul></header>
            <header><nav><ul><li>About</li></ul></nav></header>
            <footer><ul><li>Mon-Sat: 10am-11pm</li></ul></footer>
        </body></html>"#;

        let output = tag_html(html, None);

        // id chứa "menu" -> main-menu-item.
        assert!(output.contains(r#"data-editable="main-menu-item""#));
        // ul không có tín hiệu menu riêng nhưng nằm trong <nav> -> vẫn được
        // coi là menu (không phải do "trong header").
        assert_eq!(output.matches(r#"data-editable="main-menu-item""#).count(), 2);
        // ul trong footer, không id/class, không có tổ tiên nav/menu -> KHÔNG
        // được coi là menu, rơi xuống "text" như mọi thẻ có text khác.
        assert!(output.contains(r#"data-editable="text""#));
    }

    #[test]
    fn wrapper_with_no_direct_text_defers_to_its_children_instead_of_swallowing_them() {
        // SỬA LỖI QUAN TRỌNG (phát hiện qua đối chiếu file thật, không phải
        // suy luận): 1 <div> KHÔNG có text nào là CON TRỰC TIẾP của chính nó
        // (toàn bộ text nằm trong các thẻ con) phải bị BỎ QUA ở Ưu tiên 11,
        // để CHÍNH các thẻ con (span/em rỗng, h2, p) tự đứng ra tag riêng —
        // đúng theo auto_tagger.py (`if not direct_text and len(child_elements)
        // > 0: continue`). Cấu trúc dưới đây dựng lại NGUYÊN VĂN từ 1 ca lỗi
        // thật gặp trên trang thực tế: <div class="main_title"><span><em>
        // </em></span><h2>...</h2><p>...</p></div> — bản lỗi cũ gộp cả div
        // thành 1 "text" duy nhất (nuốt mất h2+p vào 1 id), thay vì 3 kết quả
        // riêng biệt: span/em rỗng KHÔNG được tag (không có text nào cả), h2
        // và p mỗi cái 1 id text riêng.
        let html = r#"<html><body>
            <div class="main_title">
                <span><em></em></span>
                <h2>Some words about us</h2>
                <p>Cum doctus civibus efficiantur in imperdiet deterruisset.</p>
            </div>
        </body></html>"#;

        let output = tag_html(html, None);

        // div KHÔNG được tag "text" (không có text riêng, chỉ là container).
        assert!(!output.contains(r#"<div class="main_title" data-builder-id"#));
        // h2 và p mỗi cái nhận 1 data-builder-id "text" RIÊNG BIỆT.
        assert_eq!(output.matches(r#"data-editable="text""#).count(), 2);
        assert!(output.contains(r#"<h2 data-builder-id="page_text_1" data-editable="text">Some words about us</h2>"#));
        assert!(output.contains(r#"<p data-builder-id="page_text_2" data-editable="text">Cum doctus civibus efficiantur in imperdiet deterruisset.</p>"#));
    }

    #[test]
    fn wrapper_with_its_own_direct_text_still_gets_tagged_as_one_block() {
        // Đối chứng cho test trên: nếu div CÓ text trực tiếp của riêng nó
        // (dù vẫn có thêm thẻ con khác), nó vẫn được tag như 1 khối — chỉ
        // trường hợp KHÔNG có text riêng nào mới bị bỏ qua.
        let html = r#"<html><body>
            <div>Xin <b>chao</b> ban</div>
        </body></html>"#;

        let output = tag_html(html, None);

        assert!(output.contains(r#"<div data-builder-id="page_text_1" data-editable="text">Xin <b>chao</b> ban</div>"#));
    }

    #[test]
    fn meta_name_matching_is_case_insensitive_like_python_lower() {
        // SỬA LỖI: auto_tagger.py hạ chữ thường `name` TRƯỚC khi so sánh với
        // 'description'/'keywords' — <meta name="Description"> (viết hoa chữ
        // đầu) vẫn phải được nhận diện, không chỉ đúng "description" thường.
        let html = r#"<html><head>
            <meta name="Description" content="Mo ta trang">
            <meta name="KEYWORDS" content="a, b, c">
        </head><body></body></html>"#;

        let output = tag_html(html, None);

        assert!(output.contains(r#"data-editable="seo-meta-description""#));
        assert!(output.contains(r#"data-editable="seo-meta-keywords""#));
    }

    #[test]
    fn attribute_editable_content_gets_data_editable_attrs() {
        let html = r#"<html><head>
            <meta name="description" content="Mo ta trang">
        </head><body>
            <img src="photo.png" alt="Anh minh hoa">
        </body></html>"#;

        let output = tag_html(html, None);

        assert!(output.contains(r#"data-editable="seo-meta-description""#));
        assert!(output.contains(r#"data-editable-attrs="content""#));
        // src không chứa "logo" -> phân loại "image" thường, không phải logo.
        assert!(output.contains(r#"data-editable="image""#));
        assert!(output.contains(r#"data-editable-attrs="alt""#));
    }

    #[test]
    fn empty_link_gets_min_size_style() {
        // Link chỉ bọc <img>, không có text -> cần style chống co về 0px.
        // Link CÓ text ("About") thì không cần.
        let html = r#"<html><body>
            <a href="/"><img src="logo.png" alt="Logo"></a>
            <a href="/about">About</a>
        </body></html>"#;

        let output = tag_html(html, None);

        // Đúng 1 link cần style (link bọc ảnh) trong 2 link — không phải cả 2.
        assert_eq!(output.matches("min-width:22px").count(), 1);
    }

    #[test]
    fn append_id_suffix_appends_to_every_builder_id() {
        let html = r#"<html><body>
            <p data-builder-id="page_text_1" data-editable="text">A</p>
            <a href="/x" data-builder-id="page_link_1" data-editable="link">B</a>
        </body></html>"#;

        let output = append_id_suffix(html, "_en");

        assert!(output.contains(r#"data-builder-id="page_text_1_en""#));
        assert!(output.contains(r#"data-builder-id="page_link_1_en""#));
        // Không còn id KHÔNG có hậu tố sót lại.
        assert!(!output.contains(r#"data-builder-id="page_text_1""#));
    }

    #[test]
    fn counter_detected_via_data_attribute_not_just_class() {
        let html = r#"<html><body>
            <span data-purecounter-end="500">0</span>
        </body></html>"#;

        let output = tag_html(html, None);

        assert!(output.contains(r#"data-editable="counter""#));
    }

    #[test]
    fn icon_requires_known_prefix_token_not_just_substring() {
        let html = r#"<html><body>
            <i class="fa-solid fa-heart">x</i>
            <button class="header-icon-btn">Click</button>
        </body></html>"#;

        let output = tag_html(html, None);

        assert!(output.contains(r#"data-editable="icon""#));
        // "header-icon-btn" chứa chuỗi con "icon" nhưng KHÔNG phải token bắt
        // đầu bằng tiền tố icon-font hợp lệ nào -> không được coi là icon
        // (button này có text "Click" nên vẫn được tag, nhưng là "button").
        assert!(output.contains(r#"data-editable="button""#));
    }

    #[test]
    fn phone_and_email_links_get_global_type() {
        let html = r#"<html><body>
            <a href="tel:+84123456789">+84 123 456 789</a>
            <a href="mailto:hi@example.com">hi@example.com</a>
        </body></html>"#;

        let output = tag_html(html, None);

        assert!(output.contains(r#"data-global-type="phone""#));
        assert!(output.contains(r#"data-global-type="email""#));
    }

    #[test]
    fn address_block_gets_text_type_with_address_global_type() {
        // SỬA LỖI (phát hiện qua đối chiếu file thật): classify() nhận diện
        // đúng đây là khối address ở Ưu tiên 8, trả về ElementType::Text
        // (khớp auto_tagger.py — address dùng CHUNG data-editable="text" với
        // text thường), nhưng trước đây KHÔNG có bước nào gắn lại
        // data-global-type="address" — detect_global_type chỉ xử lý Link/Map,
        // bỏ sót nhánh Text-vì-address. Dựng lại đúng cấu trúc thật gây lỗi:
        // heading "Address" đứng ngay TRƯỚC <p>, kích hoạt is_address_block
        // qua điều kiện "heading liền trước chứa chữ address".
        let html = r#"<html><body>
            <h3>Address</h3>
            <p>123 Main St, City</p>
        </body></html>"#;

        let output = tag_html(html, None);

        // Vẫn là "text" (KHÔNG phải 1 type riêng tên "address") — chỉ khác
        // ở data-global-type đi kèm.
        assert!(output.contains(r#"<p data-builder-id="page_text_2" data-editable="text" data-global-type="address">123 Main St, City</p>"#));
    }

    #[test]
    fn multiple_editable_attrs_are_comma_joined() {
        // 1 link vừa không có text (chỉ bọc icon) vừa có title -> cần dịch
        // CẢ title (data-editable-attrs) — không có alt/placeholder/content
        // nên chỉ có đúng 1 giá trị trong danh sách, nhưng cơ chế nối bằng
        // dấu phẩy được test riêng bằng cách kiểm tra format chung.
        let html = r#"<html><body>
            <input placeholder="Ten cua ban" title="Nhap ten" />
        </body></html>"#;

        let output = tag_html(html, None);

        // Cả placeholder VÀ title cùng có mặt -> nối bằng dấu phẩy.
        assert!(output.contains(r#"data-editable-attrs="placeholder,title""#));
    }

    #[test]
    fn button_without_text_and_checkbox_input_are_skipped() {
        let html = r#"<html><body>
            <button><svg></svg></button>
            <input type="checkbox" id="agree" />
            <input type="text" placeholder="Email" />
        </body></html>"#;

        let output = tag_html(html, None);

        assert!(!output.contains(r#"data-editable="button""#));
        // Chỉ đúng 1 input được tag (input text có placeholder) — checkbox bị bỏ qua.
        assert_eq!(output.matches(r#"data-editable="input""#).count(), 1);
    }
}
