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
        let Some(raw_stem) = file.path.file_stem().and_then(|s| s.to_str()) else {
            report
                .errors
                .push(format!("{}: tên file không hợp lệ", file.path.display()));
            continue;
        };
        // SỬA LỖI: nếu file New được thả vào ĐÃ MANG SẴN hậu tố từ 1 lần
        // export trước đó (vd người dùng lỡ thả "index_en.html" — chính file
        // đã xuất trước đây — làm New để xử lý tiếp), stem gốc "index_en"
        // cộng thêm hậu tố ngôn ngữ ĐANG xuất sẽ ra "index_en_en.html". Bóc
        // hậu tố cũ (nếu có) khỏi stem TRƯỚC khi dùng làm gốc đặt tên.
        let stem = strip_existing_filename_suffix(raw_stem);
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

/// Bóc hậu tố phiên bản `_vN` (unique_path tự thêm khi tránh ghi đè) rồi hậu
/// tố ngôn ngữ `_en/_vi/_zh/_ja` (Language::id_suffix) — nếu CÓ — khỏi 1 tên
/// file GỐC, để tính đúng TÊN GỐC THẬT SỰ trước khi nối hậu tố ngôn ngữ MỚI
/// vào lúc xuất file.
///
/// LẶP tới khi hết hậu tố (không chỉ 1 lớp) để tự phục hồi cả file đã lỡ bị
/// nhân đôi hậu tố TỪ TRƯỚC khi có fix này (vd "index_en_en" -> "index"),
/// cùng nguyên lý với `tagger::detect::strip_known_lang_suffix` (áp dụng cho
/// data-builder-id) — 2 hàm xử lý 2 thứ khác nhau (tên file vs id) nên đặt
/// riêng, không dùng chung, nhưng cùng 1 nguyên lý.
///
/// GIỮ NGUYÊN chữ hoa/thường của phần còn lại — khác `state::match_key` (hạ
/// hết về chữ thường để SO SÁNH khi khớp New<->Old) — ở đây kết quả dùng làm
/// TÊN FILE THẬT nên không được tự ý đổi case.
fn strip_existing_filename_suffix(stem: &str) -> &str {
    let mut result = stem;
    loop {
        let before = result;

        if let Some(idx) = result.rfind("_v") {
            let digits = &result[idx + 2..];
            if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                result = &result[..idx];
            }
        }

        for lang in Language::ALL {
            let suffix = lang.id_suffix();
            if result.len() > suffix.len()
                && result[result.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
            {
                result = &result[..result.len() - suffix.len()];
                break;
            }
        }

        if result == before {
            break;
        }
    }
    result
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_existing_filename_suffix_removes_language_suffix() {
        assert_eq!(strip_existing_filename_suffix("index"), "index");
        assert_eq!(strip_existing_filename_suffix("index_en"), "index");
        assert_eq!(strip_existing_filename_suffix("index_vi"), "index");
    }

    #[test]
    fn strip_existing_filename_suffix_removes_version_then_language_suffix() {
        // Đúng thứ tự thật sự xuất hiện trong tên file do unique_path sinh
        // ra: hậu tố ngôn ngữ trước (lúc save_all), _vN sau (lúc trùng tên).
        assert_eq!(strip_existing_filename_suffix("index_en_v2"), "index");
        assert_eq!(strip_existing_filename_suffix("index_en_v10"), "index");
    }

    #[test]
    fn strip_existing_filename_suffix_recovers_from_an_already_doubled_name() {
        // Ca thực tế người dùng báo lỗi: file đã lỡ bị nhân đôi hậu tố TỪ
        // TRƯỚC khi có fix này — vẫn phải tự phục hồi về đúng tên gốc.
        assert_eq!(strip_existing_filename_suffix("index_en_en"), "index");
    }

    #[test]
    fn strip_existing_filename_suffix_leaves_unrelated_names_alone() {
        // Không được nhầm 1 tên file thật sự kết thúc bằng cụm giống hậu tố
        // (vd "garden" không kết thúc bằng "_en" nên không bị cắt).
        assert_eq!(strip_existing_filename_suffix("garden"), "garden");
        assert_eq!(strip_existing_filename_suffix("hero_video"), "hero_video");
    }
}
