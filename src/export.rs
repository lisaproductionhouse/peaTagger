use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::state::{AppState, FileRole, Language};

pub struct SaveReport {
    pub saved_paths: Vec<PathBuf>,
    pub errors: Vec<String>,
}

impl SaveReport {
    /// 1 dòng tóm tắt để hiện ở status_message — panel Phần 1 chỉ có chỗ
    /// cho 1 dòng text, nên không tách saved/errors thành 2 vùng UI riêng.
    pub fn summary(&self) -> String {
        if self.saved_paths.is_empty() && self.errors.is_empty() {
            return "Chưa có gì để lưu — chọn ít nhất 1 ngôn ngữ output ở panel dưới và nạp file trước."
                .to_string();
        }
        let mut msg = format!("Đã lưu {} file.", self.saved_paths.len());
        if !self.errors.is_empty() {
            msg.push_str(&format!(" {} lỗi: {}", self.errors.len(), self.errors.join("; ")));
        }
        msg
    }
}

/// Lặp qua mọi file role = New, với mỗi ngôn ngữ output đã tích, ghi
/// `translated_by_lang` (đã gồm cả hậu tố ID nếu bật — xem
/// `AppState::rebuild_pipeline`, PREVIEW và FILE LƯU RA luôn khớp nhau vì
/// cùng đọc từ 1 nguồn) ra đĩa tại thư mục của file gốc.
///
/// Đồng bộ, không thread riêng: đây là ghi file cục bộ (khác cuộc gọi API
/// dịch ở Phần 4), thường chỉ vài chục ms cho vài chục file — chấp nhận được
/// để block UI 1 nhịp ngắn, không đáng để thêm độ phức tạp threading.
///
/// SỬA LỖI KIẾN TRÚC: `auto_tagger.py` dịch qua API HOÀN TOÀN ĐỒNG BỘ ngay
/// trong lúc Lưu (chờ từng request mạng xong mới đi tiếp), nên thao tác Lưu
/// bên đó "chạy lâu" nhưng LUÔN cho ra file HOÀN CHỈNH. Bản Rust dịch qua
/// API ở 1 thread NỀN riêng (để không đứng hình UI trong lúc chờ mạng) —
/// nhưng trước đây save_all() KHÔNG kiểm tra xem thread nền đó đã dịch xong
/// chưa, cứ dùng NGAY bất cứ gì đang có trong `translated_by_lang` tại đúng
/// thời điểm bấm Lưu. Nếu bấm Lưu ngay sau khi thả file/bật "Dùng API dịch
/// online" (trước khi thread nền kịp trả kết quả), file xuất ra sẽ lẫn lộn
/// phần đã dịch và phần CÒN NGUYÊN TIẾNG ANH một cách ÂM THẦM, không cảnh
/// báo gì — đúng triệu chứng quan sát được: Rust xuất file gần như ngay tức
/// thì, trong khi Python luôn phải đợi. Giờ CHẶN lưu (thay vì lưu thiếu
/// trong im lặng) khi còn đang dịch dở, báo rõ để người dùng đợi thêm rồi
/// bấm lại — xem thêm `Translator::has_pending_translations`.
pub fn save_all(state: &AppState) -> SaveReport {
    let mut report = SaveReport {
        saved_paths: Vec::new(),
        errors: Vec::new(),
    };

    if state.config.output_languages.is_empty() {
        report
            .errors
            .push("Chưa chọn ngôn ngữ output nào".to_string());
        return report;
    }

    if state.config.use_online_translation_api && state.translator.has_pending_translations() {
        report.errors.push(
            "Còn bản dịch đang chờ API trả kết quả — đợi vài giây rồi bấm Lưu lại (tránh xuất file thiếu bản dịch)."
                .to_string(),
        );
        return report;
    }

    let mut used_paths: HashSet<PathBuf> = HashSet::new();

    for file in state.files.iter().filter(|f| f.role == FileRole::New) {
        if file.error.is_some() {
            continue; // file lỗi đọc từ đầu, không có gì để lưu
        }
        let Some(dir) = file.path.parent() else {
            report
                .errors
                .push(format!("{}: không xác định được thư mục", file.path.display()));
            continue;
        };
        let Some(stem) = file.path.file_stem().and_then(|s| s.to_str()) else {
            report
                .errors
                .push(format!("{}: tên file không hợp lệ", file.path.display()));
            continue;
        };
        let ext = file.path.extension().and_then(|e| e.to_str()).unwrap_or("html");

        for lang in Language::ALL {
            if !state.config.output_languages.contains(&lang) {
                continue;
            }
            let Some(html) = file.translated_by_lang.get(&lang) else {
                // Chưa có bản dịch sẵn sàng cho ngôn ngữ này (vd đang chờ API
                // trả kết quả) — bỏ qua, không lưu file rỗng/thiếu dữ liệu.
                report.errors.push(format!(
                    "{} ({}): chưa có bản dịch sẵn sàng, thử lại sau",
                    file.path.display(),
                    lang.label()
                ));
                continue;
            };

            let file_suffix = format!("_{}", lang.label().to_lowercase());
            let out_path = unique_path(dir, stem, &file_suffix, ext, &mut used_paths);

            match std::fs::write(&out_path, html) {
                Ok(()) => report.saved_paths.push(out_path),
                Err(e) => report.errors.push(format!("{}: {e}", out_path.display())),
            }
        }
    }

    report
}

/// `{stem}{suffix}.{ext}`, nếu đã tồn tại (trên đĩa HOẶC đã được chọn trong
/// chính lượt lưu này — vd 2 file nguồn khác tên nhưng cùng stem) thì tăng
/// dần `_v2`, `_v3`... tới khi tìm được tên chưa dùng.
fn unique_path(dir: &Path, stem: &str, suffix: &str, ext: &str, used: &mut HashSet<PathBuf>) -> PathBuf {
    let base = format!("{stem}{suffix}");
    let mut candidate = dir.join(format!("{base}.{ext}"));
    let mut version = 2;
    while candidate.exists() || used.contains(&candidate) {
        candidate = dir.join(format!("{base}_v{version}.{ext}"));
        version += 1;
    }
    used.insert(candidate.clone());
    candidate
}
