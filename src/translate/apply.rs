use ego_tree::NodeId;
use scraper::{Html, Node, Selector};
use std::sync::LazyLock;

static TAGGED_SELECTOR: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("[data-editable]").expect("selector tĩnh hợp lệ"));

/// Duyệt mọi thẻ đã được Phần 3 gắn `data-editable`, gọi `resolve` cho từng
/// đoạn text/giá trị attribute cần dịch, trả về HTML mới.
///
/// Model THỐNG NHẤT đúng theo auto_tagger.py: với MỖI thẻ đã tag, kiểm tra
/// ĐỘC LẬP cả 5 attribute (placeholder/alt/title/content/value) — 1 thẻ
/// hoàn toàn có thể vừa có text con vừa có title cần dịch cùng lúc.
///
/// QUAN TRỌNG — đã xác minh lại trực tiếp từ auto_tagger.py
/// (`translate_html_content`, cơ chế `tag_stack[-1]`): chỉ text node là CON
/// TRỰC TIẾP của thẻ đã tag mới được dịch — trạng thái "editable" KHÔNG lan
/// truyền xuống thẻ con chưa có data-editable riêng. Vd `<p data-editable=
/// "text">Xin <b>chào</b> bạn</p>` — CHỈ "Xin " và " bạn" (con trực tiếp của
/// p) được dịch; "chào" (nằm trong <b>, <b> không tự có data-editable vì đã
/// bị loại bởi is_nested_in_text_block ở detect.rs) giữ nguyên bản gốc,
/// không dịch. Thiết kế trước của mình (duyệt toàn bộ descendants) là suy
/// luận SAI — đã sửa lại cho khớp đúng bản gốc.
///
/// An toàn cấu trúc bằng thiết kế: hàm này KHÔNG BAO GIỜ thêm/xoá/di chuyển
/// node — chỉ gán lại string bên trong 1 text node đã tồn tại sẵn, hoặc giá
/// trị 1 attribute đã tồn tại sẵn, nên <b> (hay bất kỳ thẻ con nào) luôn giữ
/// nguyên vị trí dù nội dung xung quanh nó đổi.
pub fn apply_translation(tagged_html: &str, resolve: &mut dyn FnMut(&str) -> String) -> String {
    let document = Html::parse_document(tagged_html);

    // Đọc trước (immutable) toàn bộ target cần dịch — scraper không cho vừa
    // .select() (mượn document) vừa sửa document.tree cùng lúc.
    let mut text_targets: Vec<NodeId> = Vec::new();
    let mut attr_targets: Vec<(NodeId, &'static str)> = Vec::new();

    const TRANSLATABLE_ATTRS: [&str; 5] = ["placeholder", "alt", "title", "content", "value"];

    for el in document.select(&TAGGED_SELECTOR) {
        let elem = el.value();
        for attr_name in TRANSLATABLE_ATTRS {
            if elem.attr(attr_name).is_some_and(|v| !v.trim().is_empty()) {
                attr_targets.push((el.id(), attr_name));
            }
        }
        // CHỈ con TRỰC TIẾP (el.children()), không phải el.descendants() —
        // xem giải thích ở doc comment trên hàm.
        for child in el.children() {
            if let Node::Text(text) = child.value() {
                if !text.text.trim().is_empty() {
                    text_targets.push(child.id());
                }
            }
        }
    }

    // Từ đây document không còn bị .select() mượn nữa nên mutate trực tiếp
    // qua document.tree (public field) được — TreeSink của Phần 3 không dùng
    // ở đây vì add_attrs_if_missing() sẽ KHÔNG ghi đè attribute đã có sẵn,
    // trong khi dịch content/placeholder/alt cần OVERWRITE giá trị cũ.
    let mut document = document;

    for node_id in text_targets {
        let Some(mut node_mut) = document.tree.get_mut(node_id) else {
            continue;
        };
        if let Node::Text(text) = node_mut.value() {
            let translated = resolve(&text.text);
            text.text = translated.into();
        }
    }

    for (node_id, attr_name) in attr_targets {
        let Some(mut node_mut) = document.tree.get_mut(node_id) else {
            continue;
        };
        if let Node::Element(element) = node_mut.value() {
            let Some(current) = element.attr(attr_name) else {
                continue;
            };
            let translated = resolve(current);
            // element.attrs là Vec<(QualName, StrTendril)> — xem ghi chú
            // tương tự ở tagger/mod.rs::append_id_suffix. So theo LocalName
            // (tên attribute) thay vì so cả QualName để không phụ thuộc
            // namespace/prefix.
            let target_local = html5ever::LocalName::from(attr_name);
            for (name, value) in element.attrs.iter_mut() {
                if name.local == target_local {
                    *value = translated.into();
                    break;
                }
            }
        }
    }

    document.html()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_direct_text_children_are_translated_not_nested_untagged_tags() {
        let html = r#"<html><body>
            <p data-builder-id="x" data-editable="text">Xin <b>chao</b> ban</p>
        </body></html>"#;

        let mut seen: Vec<String> = Vec::new();
        let output = apply_translation(html, &mut |text: &str| {
            seen.push(text.to_string());
            format!("[{text}]")
        });

        // <b> vẫn còn nguyên, không bị gộp/xoá/dịch chuyển.
        assert!(output.contains("<b>"));
        assert!(output.contains("</b>"));
        // CHỈ 2 mẩu text là CON TRỰC TIẾP của <p> ("Xin ", " ban") được
        // resolve() gọi tới — đúng theo cơ chế tag_stack[-1] của bản gốc
        // (auto_tagger.py): trạng thái "editable" KHÔNG lan xuống <b> vì nó
        // chưa có data-editable riêng.
        assert_eq!(seen, vec!["Xin ", " ban"]);
        // "chao" (trong <b>, chưa tag riêng) giữ NGUYÊN bản gốc, không dịch.
        assert!(output.contains(">chao<"));
        assert!(!output.contains("[chao]"));
    }

    #[test]
    fn translates_meta_content_attribute_unconditionally() {
        // Không còn cần data-editable-attrs để BIẾT phải dịch content — giờ
        // content (như placeholder/alt/title/value) được kiểm tra ĐỘC LẬP
        // trên mọi thẻ đã tag, có mặt là dịch, đúng model bản gốc.
        let html = r#"<html><head>
            <meta name="description" content="Mo ta trang" data-editable="seo-meta-description" data-builder-id="x">
        </head><body></body></html>"#;

        let output = apply_translation(html, &mut |text: &str| {
            assert_eq!(text, "Mo ta trang");
            "Page description".to_string()
        });

        assert!(output.contains(r#"content="Page description""#));
    }

    #[test]
    fn skips_elements_without_data_editable() {
        let html = r#"<html><body><p>Khong duoc tag, khong nen bi dich</p></body></html>"#;
        let output = apply_translation(html, &mut |_| "SHOULD NOT APPEAR".to_string());
        assert!(!output.contains("SHOULD NOT APPEAR"));
    }
}
