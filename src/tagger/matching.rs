use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use ego_tree::NodeId;
use regex::Regex;

use super::detect::{Candidate, OldCandidate};

/// Trích "base_name" (phần trước dấu "_SỐ" cuối cùng) từ 1 data-builder-id đã
/// có — đúng auto_tagger.py (regex `^(.*)_(\d+)$` áp TRỰC TIẾP lên chuỗi id,
/// không phải tự ráp lại từ section+type) — dùng để seed bộ đếm sinh id mới.
/// Đã đối chiếu hành vi với bản gốc trên nhiều ca biên (id không có số, có
/// nhiều cụm "_số" lồng nhau, số có số 0 ở đầu...), xem verify_counter.py.
static TRAILING_NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(.*)_(\d+)$").expect("regex tĩnh hợp lệ"));

/// So khớp candidate NEW với phần tử ĐÃ TAG trong file OLD, rồi sinh id mới
/// cho phần còn lại — mô phỏng lại ĐÚNG thuật toán `auto_tagger.py`
/// (`tag_html_content` + `get_unique_id`), khác hẳn cách tiếp cận 3 tầng
/// (identity key -> text -> vị trí) trước đây của module này. Toàn bộ phần
/// so khớp dưới đây đã được đối chiếu trực tiếp với `difflib.SequenceMatcher`
/// thật của Python trên nhiều kịch bản trước khi viết (xem verify_lcs.py).
///
/// TÓM TẮT thuật toán (đúng theo bản gốc):
///
/// 1. Với MỖI `tag_type` (giá trị `data-editable`): lấy danh sách NEW (theo
///    đúng thứ tự xuất hiện trong tài liệu) và danh sách OLD (đọc thẳng từ
///    file cũ qua `detect::extract_old_candidates` — CHỈ những phần tử đã có
///    sẵn CẢ `data-builder-id` lẫn `data-editable`, không re-classify).
/// 2. Tính "sig" (chữ ký) cho mỗi phần tử theo `detect::compute_signature` —
///    công thức phụ thuộc THẺ HTML gốc, y hệt `auto_tagger.py::get_sig`.
/// 3. Tìm dãy con chung dài nhất (LCS) giữa 2 dãy sig — mô phỏng các mốc neo
///    'equal' của `difflib.SequenceMatcher`. Các cặp LCS được ghép trực tiếp
///    (sig đã CHẮC CHẮN bằng nhau ở 2 mốc neo liên tiếp).
/// 4. Phần "khoảng trống" giữa 2 mốc LCS liền kề (hoặc trước mốc đầu / sau
///    mốc cuối) — mô phỏng opcode 'replace': nếu CẢ 2 phía (old/new) còn
///    phần tử, ghép theo ĐÚNG VỊ TRÍ tương đối bên trong khoảng trống đó,
///    KHÔNG cần sig khớp. Đây là lý do 1 link đổi hết text nhưng giữ nguyên
///    href (hoặc ngược lại) vẫn có thể kế thừa được id dù sig không khớp
///    tuyệt đối — miễn nó là phần tử duy nhất còn lại ở vị trí tương ứng.
///    Phía dư ra (khi 2 bên khoảng trống lệch độ dài) KHÔNG được gán — giống
///    hệt 'delete' (OLD dư, đơn giản không dùng tới) / 'insert' (NEW dư, chờ
///    sinh id mới ở bước 5).
/// 5. Phần NEW nào không khớp được ở bước 3-4 sẽ được sinh id mới, với bộ
///    đếm khởi tạo từ SỐ LỚN NHẤT từng xuất hiện cho cùng base_name trong
///    TOÀN BỘ file OLD — kể cả phần tử OLD không còn khớp được với bất kỳ
///    phần tử NEW nào trong lượt này (vd nội dung đã bị xoá). Số ID không
///    bao giờ bị "tái sử dụng" chỉ vì 1 phần tử cũ đã biến mất khỏi nội dung
///    mới — khớp đúng `get_unique_id`/`id_counters` của bản gốc. Đây là lỗi
///    hay gặp nhất khi so 2 bản: Rust cũ luôn bắt đầu đếm lại từ 1 cho mỗi
///    (section, type), Python thì tiếp tục từ lịch sử.
///
/// LƯU Ý (chấp nhận được): khi có NHIỀU HƠN 1 dãy con chung dài nhất khả dĩ
/// (sig trùng lặp y hệt nhau giữa nhiều phần tử), kết quả có thể lệch nhẹ so
/// với thuật toán Ratcliff/Obershelp cụ thể của Python ở đúng những trường
/// hợp mơ hồ đó (đã kiểm chứng: 10/11 kịch bản thử nghiệm khớp tuyệt đối với
/// difflib thật, ca lệch duy nhất là sig trùng lặp hoàn toàn — bản thân dữ
/// liệu đã không phân biệt được 2 phần tử nên không có "đáp án đúng duy
/// nhất"). Không đáng để tự cài lại nguyên bộ Ratcliff/Obershelp chỉ để vá
/// góc cạnh hiếm gặp này.
pub fn assign_ids(new_candidates: &[Candidate], old_candidates: &[OldCandidate]) -> HashMap<NodeId, String> {
    let new_by_type = group_new_by_type(new_candidates);
    let old_by_type = group_old_by_type(old_candidates);

    let mut assignments: HashMap<NodeId, String> = HashMap::new();
    for (&type_str, new_items) in &new_by_type {
        let empty: Vec<&OldCandidate> = Vec::new();
        let old_items = old_by_type.get(type_str).unwrap_or(&empty);
        for (node_id, id) in match_within_type(new_items, old_items) {
            assignments.insert(node_id, id);
        }
    }

    // Phần chưa khớp -> sinh id mới. Bộ đếm seed từ TOÀN BỘ id cũ (không chỉ
    // những cái vừa được kế thừa ở trên) — đúng theo get_unique_id/
    // id_counters của bản gốc: base_name lấy trực tiếp từ CHUỖI id cũ (qua
    // TRAILING_NUMBER), không phải tự ráp lại "{section}_{type}" của riêng
    // Rust, để không phụ thuộc việc section có được tính giống hệt lúc phần
    // tử đó từng được gắn hay không.
    let mut taken: HashSet<String> = assignments.values().cloned().collect();
    let mut counters: HashMap<String, usize> = HashMap::new();
    for old in old_candidates {
        seed_counter(&old.existing_builder_id, &mut counters);
    }

    for c in new_candidates {
        if assignments.contains_key(&c.node_id) {
            continue;
        }
        let base = format!("{}_{}", c.section, c.element_type.as_str());
        let new_id = loop {
            let n = counters.entry(base.clone()).or_insert(0);
            *n += 1;
            let candidate_id = format!("{base}_{n}");
            if !taken.contains(&candidate_id) {
                break candidate_id;
            }
        };
        taken.insert(new_id.clone());
        assignments.insert(c.node_id, new_id);
    }

    assignments
}

/// Cập nhật `counters[base_name]` lên số LỚN NHẤT từng thấy cho base_name đó
/// — bỏ qua im lặng nếu `existing_id` không có dạng "..._SỐ" ở cuối (id lạ/
/// chỉnh tay), giống hệt việc bản gốc chỉ cập nhật `id_counters` khi regex
/// khớp.
fn seed_counter(existing_id: &str, counters: &mut HashMap<String, usize>) {
    let Some(caps) = TRAILING_NUMBER.captures(existing_id) else {
        return;
    };
    let Ok(num) = caps[2].parse::<usize>() else {
        return;
    };
    let entry = counters.entry(caps[1].to_string()).or_insert(0);
    if num > *entry {
        *entry = num;
    }
}

fn group_new_by_type(candidates: &[Candidate]) -> HashMap<&'static str, Vec<&Candidate>> {
    let mut map: HashMap<&'static str, Vec<&Candidate>> = HashMap::new();
    for c in candidates {
        map.entry(c.element_type.as_str()).or_default().push(c);
    }
    map
}

fn group_old_by_type(candidates: &[OldCandidate]) -> HashMap<&str, Vec<&OldCandidate>> {
    let mut map: HashMap<&str, Vec<&OldCandidate>> = HashMap::new();
    for c in candidates {
        map.entry(c.tag_type.as_str()).or_default().push(c);
    }
    map
}

/// So khớp 1 nhóm cùng tag_type — xem giải thích thuật toán ở `assign_ids`.
fn match_within_type(new_items: &[&Candidate], old_items: &[&OldCandidate]) -> Vec<(NodeId, String)> {
    let old_sigs: Vec<&str> = old_items.iter().map(|c| c.sig.as_str()).collect();
    let new_sigs: Vec<&str> = new_items.iter().map(|c| c.sig.as_str()).collect();
    let pairs = lcs_pairs(&old_sigs, &new_sigs);

    let mut result = Vec::new();
    let mut prev_old = 0usize;
    let mut prev_new = 0usize;

    for &(oi, ni) in &pairs {
        assign_gap(old_items, new_items, prev_old, oi, prev_new, ni, &mut result);
        // Mốc neo 'equal': sig đã CHẮC CHẮN bằng nhau, gán trực tiếp.
        result.push((new_items[ni].node_id, old_items[oi].existing_builder_id.clone()));
        prev_old = oi + 1;
        prev_new = ni + 1;
    }
    assign_gap(old_items, new_items, prev_old, old_items.len(), prev_new, new_items.len(), &mut result);

    result
}

/// Ghép theo VỊ TRÍ 1 khoảng trống giữa 2 mốc neo LCS (hoặc trước mốc đầu /
/// sau mốc cuối) — mô phỏng opcode 'replace' của `difflib.SequenceMatcher`:
/// 2 phía còn phần tử tới đâu, ghép theo đúng thứ tự tới đó; phía dư (khi 2
/// bên lệch độ dài) không được gán — bên NEW dư sẽ tự sinh id mới ở
/// `assign_ids`, bên OLD dư đơn giản không dùng tới (giống hệt 'delete').
fn assign_gap(
    old_items: &[&OldCandidate],
    new_items: &[&Candidate],
    old_start: usize,
    old_end: usize,
    new_start: usize,
    new_end: usize,
    result: &mut Vec<(NodeId, String)>,
) {
    let old_slice = &old_items[old_start..old_end];
    let new_slice = &new_items[new_start..new_end];
    for (o, n) in old_slice.iter().zip(new_slice.iter()) {
        result.push((n.node_id, o.existing_builder_id.clone()));
    }
}

/// Dãy con chung dài nhất (LCS) giữa 2 slice chuỗi, trả về danh sách cặp chỉ
/// số `(old_idx, new_idx)` theo thứ tự tăng dần cả 2 chiều — mô phỏng các mốc
/// neo 'equal' của `difflib.SequenceMatcher`. Cài bằng quy hoạch động
/// O(n*m): số phần tử cùng 1 tag_type trên 1 trang thực tế chỉ vài chục tới
/// vài trăm, không đáng lo hiệu năng.
fn lcs_pairs(old_sigs: &[&str], new_sigs: &[&str]) -> Vec<(usize, usize)> {
    let n = old_sigs.len();
    let m = new_sigs.len();
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if old_sigs[i] == new_sigs[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }

    let mut pairs = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if old_sigs[i] == new_sigs[j] {
            pairs.push((i, j));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_counter_reads_trailing_number_like_python_regex() {
        let mut counters = HashMap::new();
        seed_counter("page_text_1", &mut counters);
        seed_counter("page_text_10", &mut counters);
        seed_counter("page_text_2", &mut counters); // nhỏ hơn max hiện tại -> không hạ xuống
        assert_eq!(counters.get("page_text"), Some(&10));

        // Id không có dạng "..._SỐ" -> bỏ qua, không panic, không tạo entry rác.
        let mut counters2 = HashMap::new();
        seed_counter("khong_co_so", &mut counters2);
        assert!(counters2.is_empty());
    }

    #[test]
    fn lcs_pairs_finds_anchors_around_multiple_replace_blocks() {
        // old: A B C D E / new: A X C Y E — 3 mốc neo (A,C,E), 2 khoảng trống
        // (B->X, D->Y) đều là replace 1-1.
        let old_sigs = ["A", "B", "C", "D", "E"];
        let new_sigs = ["A", "X", "C", "Y", "E"];
        let pairs = lcs_pairs(&old_sigs, &new_sigs);
        assert_eq!(pairs, vec![(0, 0), (2, 2), (4, 4)]);
    }
}
