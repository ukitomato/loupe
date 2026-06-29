// Integration tests for build_root → search / sync_all.
// Each test owns its tempdir; tests run in parallel without conflicts.

mod common;
use common::{assert_hit, assert_no_hit, to_euc_jp, to_shift_jis, TwoRootWorkspace, Workspace};
use indexify::{search, sync_all};

// ────────────────────────────────────────────────────────────────
// Substring: basic case-insensitive / case-sensitive
// ────────────────────────────────────────────────────────────────

#[test]
fn substring_case_insensitive() {
    let ws = Workspace::new("utf-8");
    ws.write("a.txt", b"calculateTotal(x, y)");
    ws.build("utf-8");
    let hits = search(&ws.state, "calculatetotal", false, 300, false)
        .unwrap()
        .hits;
    assert_hit(&hits, "a.txt", 1);
}

#[test]
fn substring_case_insensitive_uppercase_query() {
    let ws = Workspace::new("utf-8");
    ws.write("a.txt", b"calculateTotal(x, y)");
    ws.build("utf-8");
    let hits = search(&ws.state, "CALCULATETOTAL", false, 300, false)
        .unwrap()
        .hits;
    assert_hit(&hits, "a.txt", 1);
}

#[test]
fn substring_case_sensitive_match() {
    let ws = Workspace::new("utf-8");
    ws.write("a.txt", b"calculateTotal(x, y)");
    ws.build("utf-8");
    let hits = search(&ws.state, "calculateTotal", false, 300, true)
        .unwrap()
        .hits;
    assert_hit(&hits, "a.txt", 1);
}

#[test]
fn substring_case_sensitive_no_match_wrong_case() {
    let ws = Workspace::new("utf-8");
    ws.write("a.txt", b"calculateTotal(x, y)");
    ws.build("utf-8");
    let hits = search(&ws.state, "CALCULATETOTAL", false, 300, true)
        .unwrap()
        .hits;
    assert!(hits.is_empty());
}

// ────────────────────────────────────────────────────────────────
// Query length edge cases
// ────────────────────────────────────────────────────────────────

#[test]
fn query_one_char_returns_empty() {
    let ws = Workspace::new("utf-8");
    ws.write("a.txt", b"abc");
    ws.build("utf-8");
    let hits = search(&ws.state, "a", false, 300, false).unwrap().hits;
    assert!(hits.is_empty());
}

#[test]
fn query_two_chars_returns_hit() {
    // 2-char ASCII queries are now searchable (bigram-indexed), not silently empty.
    let ws = Workspace::new("utf-8");
    ws.write("a.txt", b"abcdef");
    ws.build("utf-8");
    let hits = search(&ws.state, "ab", false, 300, false).unwrap().hits;
    assert_hit(&hits, "a.txt", 1);
}

#[test]
fn query_two_char_cjk_word_returns_hit() {
    // The motivating case: a 2-char Japanese word like 契約 was unsearchable before.
    let ws = Workspace::new("utf-8");
    ws.write("a.txt", "保険契約の状態区分\n".as_bytes());
    ws.build("utf-8");
    let hits = search(&ws.state, "契約", false, 300, false).unwrap().hits;
    assert_hit(&hits, "a.txt", 1);
}

#[test]
fn query_three_chars_returns_hit() {
    let ws = Workspace::new("utf-8");
    ws.write("a.txt", b"abcdef");
    ws.build("utf-8");
    let hits = search(&ws.state, "abc", false, 300, false).unwrap().hits;
    assert_hit(&hits, "a.txt", 1);
}

// ────────────────────────────────────────────────────────────────
// Line numbers
// ────────────────────────────────────────────────────────────────

#[test]
fn hit_on_first_line() {
    let ws = Workspace::new("utf-8");
    ws.write("a.txt", b"TARGET line\nsecond line\n");
    ws.build("utf-8");
    let hits = search(&ws.state, "TARGET", false, 300, false).unwrap().hits;
    assert_hit(&hits, "a.txt", 1);
}

#[test]
fn hit_on_last_line_no_trailing_newline() {
    let ws = Workspace::new("utf-8");
    ws.write("a.txt", b"first\nTARGET");
    ws.build("utf-8");
    let hits = search(&ws.state, "TARGET", false, 300, false).unwrap().hits;
    assert_hit(&hits, "a.txt", 2);
}

#[test]
fn multiple_lines_same_query_different_hits() {
    let ws = Workspace::new("utf-8");
    ws.write("a.txt", b"TARGET one\nother\nTARGET two\n");
    ws.build("utf-8");
    let hits = search(&ws.state, "TARGET", false, 300, false).unwrap().hits;
    assert_hit(&hits, "a.txt", 1);
    assert_hit(&hits, "a.txt", 3);
}

#[test]
fn two_occurrences_same_line_yields_one_hit() {
    let ws = Workspace::new("utf-8");
    ws.write("a.txt", b"foo foo foo\n");
    ws.build("utf-8");
    let hits = search(&ws.state, "foo", false, 300, false).unwrap().hits;
    assert_eq!(
        hits.iter()
            .filter(|h| h.file.ends_with("a.txt") && h.line == 1)
            .count(),
        1
    );
}

#[test]
fn crlf_line_ending_correct_line_number() {
    let ws = Workspace::new("utf-8");
    ws.write("a.txt", b"first\r\nTARGET\r\nthird\r\n");
    ws.build("utf-8");
    let hits = search(&ws.state, "TARGET", false, 300, false).unwrap().hits;
    assert_hit(&hits, "a.txt", 2);
}

#[test]
fn crlf_hit_text_has_no_carriage_return() {
    let ws = Workspace::new("utf-8");
    ws.write("a.txt", b"TARGET line\r\n");
    ws.build("utf-8");
    let hits = search(&ws.state, "TARGET", false, 300, false).unwrap().hits;
    assert!(!hits.is_empty());
    assert!(
        !hits[0].text.contains('\r'),
        "text should not contain \\r: {:?}",
        hits[0].text
    );
}

// ────────────────────────────────────────────────────────────────
// Encoding: Shift_JIS
// ────────────────────────────────────────────────────────────────

#[test]
fn shift_jis_japanese_query_hits() {
    let ws = Workspace::new("shift_jis");
    let content = to_shift_jis("検索テスト用ファイル\nマッチすべき行\n");
    ws.write("sjis.cbl", &content);
    ws.build("shift_jis");
    let hits = search(&ws.state, "マッチすべき", false, 300, false)
        .unwrap()
        .hits;
    assert_hit(&hits, "sjis.cbl", 2);
}

#[test]
fn shift_jis_no_false_negative_ascii_in_sjis_file() {
    let ws = Workspace::new("shift_jis");
    let content = to_shift_jis("PROCEDURE DIVISION.\n    MOVE 0 TO COUNTER.\n");
    ws.write("prog.cbl", &content);
    ws.build("shift_jis");
    let hits = search(&ws.state, "PROCEDURE", false, 300, false)
        .unwrap()
        .hits;
    assert_hit(&hits, "prog.cbl", 1);
}

// ────────────────────────────────────────────────────────────────
// Encoding: EUC-JP
// ────────────────────────────────────────────────────────────────

#[test]
fn euc_jp_japanese_query_hits() {
    let ws = Workspace::new("euc-jp");
    let content = to_euc_jp("データ処理プログラム\n結果を出力する\n");
    ws.write("eucjp.cbl", &content);
    ws.build("euc-jp");
    let hits = search(&ws.state, "データ処理", false, 300, false)
        .unwrap()
        .hits;
    assert_hit(&hits, "eucjp.cbl", 1);
}

// ────────────────────────────────────────────────────────────────
// Mixed encoding index (UTF-8 root + Shift_JIS root)
// ────────────────────────────────────────────────────────────────

#[test]
fn mixed_encoding_both_roots_hit() {
    let ws = TwoRootWorkspace::new();
    ws.write_utf8("utf8.rs", "fn calculate_total() { }");
    ws.write_sjis("sjis.cbl", "マッチする行\n計算処理\n");
    ws.build();
    // UTF-8 file hit
    let hits_en = search(&ws.state, "calculate_total", false, 300, false)
        .unwrap()
        .hits;
    assert_hit(&hits_en, "utf8.rs", 1);
    // Shift_JIS file hit
    let hits_jp = search(&ws.state, "計算処理", false, 300, false)
        .unwrap()
        .hits;
    assert_hit(&hits_jp, "sjis.cbl", 2);
}

#[test]
fn mixed_encoding_no_cross_contamination() {
    let ws = TwoRootWorkspace::new();
    ws.write_utf8("only_utf8.rs", "unique_utf8_identifier");
    ws.write_sjis("only_sjis.cbl", "固有の識別子テキスト\n");
    ws.build();
    let hits = search(&ws.state, "unique_utf8_identifier", false, 300, false)
        .unwrap()
        .hits;
    assert_hit(&hits, "only_utf8.rs", 1);
    assert_no_hit(&hits, "only_sjis.cbl");
}

// ────────────────────────────────────────────────────────────────
// COBOL/JCL style: fixed-width column text
// ────────────────────────────────────────────────────────────────

#[test]
fn cobol_fixed_width_columns_hit() {
    let ws = Workspace::new("utf-8");
    // Typical COBOL fixed-format: columns 7-72 are code
    ws.write(
        "prog.cbl",
        b"      *COMMENT\n       MOVE WS-AMOUNT TO TS-TOTAL.\n       STOP RUN.\n",
    );
    ws.build("utf-8");
    let hits = search(&ws.state, "WS-AMOUNT", false, 300, false)
        .unwrap()
        .hits;
    assert_hit(&hits, "prog.cbl", 2);
}

// ────────────────────────────────────────────────────────────────
// Regex mode
// ────────────────────────────────────────────────────────────────

#[test]
fn regex_literal_run_match() {
    let ws = Workspace::new("utf-8");
    ws.write("a.txt", b"calculateTotal(100)\n");
    ws.build("utf-8");
    let hits = search(&ws.state, r"calc.*Total", true, 300, false)
        .unwrap()
        .hits;
    assert_hit(&hits, "a.txt", 1);
}

#[test]
fn regex_with_digit_quantifier() {
    let ws = Workspace::new("utf-8");
    ws.write("a.txt", b"foo123bar\n");
    ws.build("utf-8");
    let hits = search(&ws.state, r"foo\d+bar", true, 300, false)
        .unwrap()
        .hits;
    assert_hit(&hits, "a.txt", 1);
}

#[test]
fn regex_no_literal_run_returns_err() {
    let ws = Workspace::new("utf-8");
    ws.write("a.txt", b"123\n");
    ws.build("utf-8");
    // \d{3} has no literal run of >=2 chars (only single chars sit between metacharacters)
    let result = search(&ws.state, r"\d{3}", true, 300, false);
    assert!(
        result.is_err(),
        "expected Err for regex with no literal >=2-char run"
    );
}

#[test]
fn regex_only_metachar_class_returns_err() {
    let ws = Workspace::new("utf-8");
    ws.write("a.txt", b"ABCDEF\n");
    ws.build("utf-8");
    let result = search(&ws.state, r"[A-Z]+", true, 300, false);
    assert!(result.is_err());
}

#[test]
fn regex_case_insensitive_default() {
    let ws = Workspace::new("utf-8");
    ws.write("a.txt", b"calculateTotal\n");
    ws.build("utf-8");
    let hits = search(&ws.state, "CALCULATETOTAL", true, 300, false)
        .unwrap()
        .hits;
    assert_hit(&hits, "a.txt", 1);
}

#[test]
fn regex_case_sensitive() {
    let ws = Workspace::new("utf-8");
    ws.write("a.txt", b"calculateTotal\n");
    ws.build("utf-8");
    let hits_match = search(&ws.state, "calculateTotal", true, 300, true)
        .unwrap()
        .hits;
    assert_hit(&hits_match, "a.txt", 1);
    let hits_no = search(&ws.state, "CALCULATETOTAL", true, 300, true)
        .unwrap()
        .hits;
    assert!(hits_no.is_empty());
}

#[test]
fn regex_cjk_literal_run_pre_filters() {
    // A 2-char CJK literal run (契約) now anchors the n-gram pre-filter; before, a regex with no
    // >=3-char ASCII literal returned Err.
    let ws = Workspace::new("utf-8");
    ws.write("a.txt", "契約者の登録情報\n".as_bytes());
    ws.build("utf-8");
    let hits = search(&ws.state, "契約.*情報", true, 300, false)
        .unwrap()
        .hits;
    assert_hit(&hits, "a.txt", 1);
}

// ────────────────────────────────────────────────────────────────
// Result caps
// ────────────────────────────────────────────────────────────────

#[test]
fn per_file_match_cap_50_lines() {
    let ws = Workspace::new("utf-8");
    // 60 lines each containing "TARGET"
    let content = (0..60)
        .map(|i| format!("TARGET line {i}\n"))
        .collect::<String>();
    ws.write("big.txt", content.as_bytes());
    ws.build("utf-8");
    let hits = search(&ws.state, "TARGET", false, 300, false).unwrap().hits;
    let file_hits: Vec<_> = hits
        .iter()
        .filter(|h| h.file.ends_with("big.txt"))
        .collect();
    assert_eq!(file_hits.len(), 50, "per-file cap should be 50");
}

#[test]
fn max_truncates_total_results() {
    let ws = Workspace::new("utf-8");
    for i in 0..20 {
        ws.write(&format!("f{i}.txt"), b"TARGET\n");
    }
    ws.build("utf-8");
    let hits = search(&ws.state, "TARGET", false, 5, false).unwrap().hits;
    assert!(hits.len() <= 5);
}

// ────────────────────────────────────────────────────────────────
// File filters
// ────────────────────────────────────────────────────────────────

#[test]
fn ignore_file_excludes_file() {
    // .ignore (not .gitignore) is respected by the `ignore` crate without requiring a git repo,
    // making it portable for tmpdir-based tests.
    let ws = Workspace::new("utf-8");
    std::fs::write(ws.root.join(".ignore"), "ignored.txt\n").unwrap();
    ws.write("ignored.txt", b"TARGET");
    ws.write("visible.txt", b"OTHER");
    ws.build("utf-8");
    let hits = search(&ws.state, "TARGET", false, 300, false).unwrap().hits;
    assert_no_hit(&hits, "ignored.txt");
}

#[test]
fn hidden_file_excluded_by_standard_filters() {
    let ws = Workspace::new("utf-8");
    ws.write(".env", b"SECRET=TARGET");
    ws.write("normal.txt", b"not a secret");
    ws.build("utf-8");
    let hits = search(&ws.state, "TARGET", false, 300, false).unwrap().hits;
    assert_no_hit(&hits, ".env");
}

#[test]
fn binary_file_skipped() {
    let ws = Workspace::new("utf-8");
    // file with null bytes → detected as binary → skipped
    let mut content = b"TARGET".to_vec();
    content.push(0x00);
    content.extend_from_slice(b"rest");
    ws.write("binary.bin", &content);
    ws.build("utf-8");
    let hits = search(&ws.state, "TARGET", false, 300, false).unwrap().hits;
    assert_no_hit(&hits, "binary.bin");
}

#[test]
fn oversized_file_skipped() {
    let ws = Workspace::new("utf-8");
    // 2MB + 1 byte → skipped
    let content = vec![b'A'; 2_000_001];
    ws.write("huge.txt", &content);
    ws.write("small.txt", b"AAA");
    ws.build("utf-8");
    // "AAA" is in small.txt but not in huge.txt (skipped)
    let hits = search(&ws.state, "AAA", false, 300, false).unwrap().hits;
    assert_no_hit(&hits, "huge.txt");
    assert_hit(&hits, "small.txt", 1);
}

#[test]
fn nested_directory_indexed() {
    let ws = Workspace::new("utf-8");
    ws.write("deep/nested/dir/file.txt", b"deeply_nested_identifier");
    ws.build("utf-8");
    let hits = search(&ws.state, "deeply_nested_identifier", false, 300, false)
        .unwrap()
        .hits;
    assert_hit(&hits, "file.txt", 1);
}

// ────────────────────────────────────────────────────────────────
// Sync (incremental)
// ────────────────────────────────────────────────────────────────

#[test]
fn sync_picks_up_new_file() {
    let ws = Workspace::new("utf-8");
    ws.write("existing.txt", b"initial content");
    ws.build("utf-8");
    // add a new file and sync
    ws.write("new_file.txt", b"brand_new_content");
    // ensure mtime differs
    std::thread::sleep(std::time::Duration::from_millis(10));
    sync_all(&ws.state, |_| {}).unwrap();
    let hits = search(&ws.state, "brand_new_content", false, 300, false)
        .unwrap()
        .hits;
    assert_hit(&hits, "new_file.txt", 1);
}

#[test]
fn sync_reflects_modified_file() {
    let ws = Workspace::new("utf-8");
    ws.write("f.txt", b"old content here");
    ws.build("utf-8");
    // overwrite — force mtime change by sleeping 10ms
    std::thread::sleep(std::time::Duration::from_millis(10));
    ws.write("f.txt", b"new_updated_content");
    sync_all(&ws.state, |_| {}).unwrap();
    let hits = search(&ws.state, "new_updated_content", false, 300, false)
        .unwrap()
        .hits;
    assert_hit(&hits, "f.txt", 1);
    let old_hits = search(&ws.state, "old content here", false, 300, false)
        .unwrap()
        .hits;
    assert_no_hit(&old_hits, "f.txt");
}

#[test]
fn sync_removes_deleted_file() {
    let ws = Workspace::new("utf-8");
    ws.write("to_delete.txt", b"delete_me_content");
    ws.build("utf-8");
    std::fs::remove_file(ws.root.join("to_delete.txt")).unwrap();
    sync_all(&ws.state, |_| {}).unwrap();
    let hits = search(&ws.state, "delete_me_content", false, 300, false)
        .unwrap()
        .hits;
    assert_no_hit(&hits, "to_delete.txt");
}

#[test]
fn sync_unchanged_file_not_reindexed() {
    // Regression: unchanged file (same mtime) must still be reachable after sync.
    let ws = Workspace::new("utf-8");
    ws.write("stable.txt", b"stable_unique_identifier");
    ws.build("utf-8");
    // sync without touching the file
    let stats = sync_all(&ws.state, |_| {}).unwrap();
    assert_eq!(stats.updated, 0);
    let hits = search(&ws.state, "stable_unique_identifier", false, 300, false)
        .unwrap()
        .hits;
    assert_hit(&hits, "stable.txt", 1);
}
