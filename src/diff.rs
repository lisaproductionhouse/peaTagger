use eframe::egui;
use similar::{ChangeTag, TextDiff};

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
    fn plain_variant_aligns_the_same_way_without_color() {
        // Trường hợp thực tế: html5ever gộp <!DOCTYPE html><html><head> về
        // chung 1 dòng khi serialize lại dù bản gốc mỗi thẻ 1 dòng riêng.
        let old = "<!DOCTYPE html>\n<html>\n<head>\n<title>x</title>\n";
        let new = "<!DOCTYPE html><html><head>\n<title>x</title>\n";
        let (left, right) = build_aligned_plain_jobs(old, new);
        assert_eq!(line_count(&left), line_count(&right));
    }

    #[test]
    fn wrap_is_disabled_so_long_lines_never_auto_wrap() {
        let (left, right) = build_diff_jobs("a\n", "a\n");
        assert_eq!(left.wrap.max_width, f32::INFINITY);
        assert_eq!(right.wrap.max_width, f32::INFINITY);
    }
}
