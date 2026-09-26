use eframe::egui;
use similar::{ChangeTag, TextDiff};

/// So sánh `old` (code gốc) với `new` (pending_html — bản đã gắn tag) ở cả
/// cấp DÒNG và TỪ.
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
/// Trả về (job_trái, job_phải): trái tô ĐỎ phần bị xoá, phải tô XANH phần
/// mới thêm (bao gồm data-builder-id/data-editable Phần 3 vừa gắn), phần
/// giống nhau giữ màu mặc định.
pub fn build_diff_jobs(old: &str, new: &str) -> (egui::text::LayoutJob, egui::text::LayoutJob) {
    let diff = TextDiff::from_lines(old, new);

    let font = egui::FontId::monospace(13.0);
    let default_color = egui::Color32::from_gray(220);
    let red = egui::Color32::from_rgb(255, 110, 110);
    let green = egui::Color32::from_rgb(120, 220, 140);

    let fmt = |color: egui::Color32| egui::text::TextFormat {
        font_id: font.clone(),
        color,
        ..Default::default()
    };

    let mut left = egui::text::LayoutJob::default();
    let mut right = egui::text::LayoutJob::default();

    for change in diff.iter_all_inline_changes() {
        match change.tag() {
            ChangeTag::Equal => {
                for (_, piece) in change.iter_strings_lossy() {
                    left.append(&piece, 0.0, fmt(default_color));
                    right.append(&piece, 0.0, fmt(default_color));
                }
            }
            ChangeTag::Delete => {
                for (emphasized, piece) in change.iter_strings_lossy() {
                    let color = if emphasized { red } else { default_color };
                    left.append(&piece, 0.0, fmt(color));
                }
            }
            ChangeTag::Insert => {
                for (emphasized, piece) in change.iter_strings_lossy() {
                    let color = if emphasized { green } else { default_color };
                    right.append(&piece, 0.0, fmt(color));
                }
            }
        }
    }

    (left, right)
}
