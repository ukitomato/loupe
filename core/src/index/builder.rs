// builder.rs — populate and incrementally maintain the index.
//
//   build_root  full (re)build over one root: parallel walk -> trigrams -> one writer adds docs
//   sync_all    catch-up: reindex files whose mtime changed, drop entries for deleted files
//   update_path single-path incremental update (used by the watcher)
//
// Files are decoded with their root's encoding before trigram extraction, and the encoding name is
// stored on the doc so search can re-decode the same way.

use anyhow::{anyhow, Result};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tantivy::schema::Value;
use tantivy::tokenizer::{PreTokenizedString, Token};
use tantivy::{DocAddress, TantivyDocument, Term};

use super::{Fields, State, WRITER_HEAP_BYTES};
use crate::encoding::enc_name_of;

const MAX_FILE_BYTES: u64 = 2_000_000;

fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8192).any(|&b| b == 0)
}

/// Distinct char n-grams (length 2 and 3) of the lowercased text — the candidate-filter terms
/// stored per document. Bigrams enable 2-char queries (notably 2-char CJK words like 契約);
/// trigrams stay the more selective term for >=3-char queries. Both lengths live in the same
/// field — a 2-char and a 3-char term never collide — and the field is `IndexRecordOption::Basic`
/// (no positions/freqs), so adding bigrams costs only modest extra index size.
///
/// Implemented as a sliding window over the char iterator (O(1) extra space) to avoid
/// collecting all characters into a Vec<char> (O(N)) and avoid a separate to_ascii_lowercase()
/// copy. Lowercasing is applied per-character inline.
fn distinct_ngrams(text: &str) -> Vec<String> {
    let mut set = HashSet::new();
    let mut prev2 = '\0';
    let mut prev1 = '\0';
    for (i, c) in text.chars().enumerate() {
        let c = c.to_ascii_lowercase();
        if i >= 1 {
            let mut s = String::with_capacity(8);
            s.push(prev1);
            s.push(c);
            set.insert(s);
        }
        if i >= 2 {
            let mut s = String::with_capacity(12);
            s.push(prev2);
            s.push(prev1);
            s.push(c);
            set.insert(s);
        }
        prev2 = prev1;
        prev1 = c;
    }
    set.into_iter().collect()
}

fn make_doc(
    fields: &Fields,
    path: &str,
    enc_name: &str,
    mtime: u64,
    tris: Vec<String>,
) -> TantivyDocument {
    let mut d = TantivyDocument::new();
    d.add_text(fields.path, path);
    d.add_text(fields.enc, enc_name);
    d.add_u64(fields.mtime, mtime);
    let tokens: Vec<Token> = tris
        .into_iter()
        .enumerate()
        .map(|(i, t)| Token {
            position: i,
            text: t,
            ..Default::default()
        })
        .collect();
    d.add_pre_tokenized_text(
        fields.tri,
        PreTokenizedString {
            text: String::new(),
            tokens,
        },
    );
    d
}

fn file_mtime_ms(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Read+decode a file and return (mtime_ms, distinct n-grams). None if skipped (too big / binary).
fn file_meta(path: &Path, enc: &'static encoding_rs::Encoding) -> Option<(u64, Vec<String>)> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > MAX_FILE_BYTES {
        return None;
    }
    let mt = file_mtime_ms(&meta);
    let bytes = std::fs::read(path).ok()?;
    if is_binary(&bytes) {
        return None;
    }
    let (text, _, _) = enc.decode(&bytes);
    Some((mt, distinct_ngrams(text.as_ref())))
}

/// Full (re)build over a single root. Parallel walk produces trigrams; one consumer adds docs.
pub fn build_root<F: Fn(u64) + Sync>(
    state: &State,
    root: &str,
    enc_name: &str,
    progress: F,
) -> Result<u64> {
    let enc = crate::encoding::enc_by_name(enc_name);
    let fields = state.fields;
    let enc_owned = enc_name.to_string();
    let (tx, rx) = mpsc::sync_channel::<(String, u64, Vec<String>)>(64);

    let writer_mutex = &state.writer;
    let total = std::thread::scope(|scope| -> Result<u64> {
        let consumer = scope.spawn(|| -> Result<u64> {
            let guard = writer_mutex.lock().unwrap();
            let w = guard.as_ref().expect("writer not initialized");
            let mut n = 0u64;
            for (path, mt, tris) in rx {
                // delete any existing doc for this path (safe for re-builds), then add
                w.delete_term(Term::from_field_text(fields.path, &path));
                w.add_document(make_doc(&fields, &path, &enc_owned, mt, tris))?;
                n += 1;
                if n.is_multiple_of(5000) {
                    progress(n);
                }
            }
            Ok(n)
        });

        let walker = ignore::WalkBuilder::new(root)
            .standard_filters(true)
            .build_parallel();
        walker.run(|| {
            let tx = tx.clone();
            Box::new(move |result| {
                if let Ok(entry) = result {
                    if entry.file_type().is_some_and(|t| t.is_file()) {
                        if let Some((mt, tris)) = file_meta(entry.path(), enc) {
                            let _ =
                                tx.send((entry.path().to_string_lossy().into_owned(), mt, tris));
                        }
                    }
                }
                ignore::WalkState::Continue
            })
        });
        drop(tx);
        consumer.join().map_err(|_| anyhow!("consumer panicked"))?
    })?;

    state.writer.lock().unwrap().as_mut().unwrap().commit()?;
    state.reader.reload()?; // make committed docs visible to searches immediately
    Ok(total)
}

/// Load the current { path -> mtime_ms } map from the index (live docs only).
fn load_index_mtimes(state: &State) -> Result<HashMap<String, u64>> {
    let searcher = state.reader.searcher();
    let mut map = HashMap::new();
    for (ord, seg) in searcher.segment_readers().iter().enumerate() {
        let alive = seg.alive_bitset();
        for doc_id in 0..seg.max_doc() {
            if let Some(bs) = alive {
                if !bs.is_alive(doc_id) {
                    continue;
                }
            }
            let d: TantivyDocument = searcher.doc(DocAddress::new(ord as u32, doc_id))?;
            let path = d
                .get_first(state.fields.path)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if path.is_empty() {
                continue;
            }
            let mt = d
                .get_first(state.fields.mtime)
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            map.insert(path, mt);
        }
    }
    Ok(map)
}

/// Outcome of a sync pass.
pub struct SyncStats {
    pub updated: u64,
    pub removed: u64,
}

/// Catch-up sync against the configured roots: reindex new/changed files (by mtime),
/// delete index entries whose files are gone.
///
/// The walker sends only new/changed files through a bounded channel (backpressure).
/// Deletion detection is done after the walk by checking Path::exists() — this avoids
/// accumulating all 300K paths in a `seen` HashSet.
pub fn sync_all<F: Fn(u64) + Sync>(state: &State, progress: F) -> Result<SyncStats> {
    let roots_snapshot: Vec<(String, &'static encoding_rs::Encoding)> = state
        .roots
        .lock()
        .unwrap()
        .iter()
        .map(|(p, e)| (p.to_string_lossy().into_owned(), *e))
        .collect();
    let indexed = Arc::new(load_index_mtimes(state)?);
    let fields = state.fields;
    let writer_mutex = &state.writer;
    let indexed_consumer = indexed.clone();
    // Bounded channel: only changed files are sent; backpressure keeps memory bounded.
    let (tx, rx) = mpsc::sync_channel::<(String, String, u64, Vec<String>)>(64);

    let res = std::thread::scope(|scope| -> Result<(u64, u64)> {
        let consumer = scope.spawn(|| -> Result<(u64, u64)> {
            let guard = writer_mutex.lock().unwrap();
            let w = guard.as_ref().expect("writer not initialized");
            let mut updated = 0u64;
            for (p, enc_name, mt, tris) in rx {
                w.delete_term(Term::from_field_text(fields.path, &p));
                w.add_document(make_doc(&fields, &p, &enc_name, mt, tris))?;
                updated += 1;
                if updated.is_multiple_of(5000) {
                    progress(updated);
                }
            }
            // Deletion: remove index entries for files that no longer exist on disk.
            // Files that exist but are now too large or binary stay in the index as-is.
            let mut removed = 0u64;
            for p in indexed_consumer.keys() {
                if !Path::new(p).exists() {
                    w.delete_term(Term::from_field_text(fields.path, p));
                    removed += 1;
                }
            }
            Ok((updated, removed))
        });

        for (rp, enc) in &roots_snapshot {
            let enc_name = enc_name_of(enc).to_string();
            let walker = ignore::WalkBuilder::new(rp)
                .standard_filters(true)
                .build_parallel();
            walker.run(|| {
                let tx = tx.clone();
                let indexed = indexed.clone();
                let enc_name = enc_name.clone();
                Box::new(move |result| {
                    if let Ok(entry) = result {
                        if entry.file_type().is_some_and(|t| t.is_file()) {
                            let path = entry.path();
                            let pathstr = path.to_string_lossy().into_owned();
                            if let Ok(meta) = std::fs::metadata(path) {
                                let mt = file_mtime_ms(&meta);
                                // Skip if already indexed at this exact mtime (and within size
                                // limit, so file_meta would produce the same n-grams).
                                let is_current = meta.len() <= MAX_FILE_BYTES
                                    && indexed.get(&pathstr).copied() == Some(mt);
                                if !is_current {
                                    if let Some((mt2, tris)) = file_meta(path, enc) {
                                        let _ = tx.send((
                                            pathstr,
                                            enc_name.clone(),
                                            mt2,
                                            tris,
                                        ));
                                    }
                                    // Too-large or binary: file_meta returns None → nothing sent.
                                    // Path::exists() in the consumer keeps the index entry alive.
                                }
                            }
                        }
                    }
                    ignore::WalkState::Continue
                })
            });
        }
        drop(tx);
        consumer
            .join()
            .map_err(|_| anyhow!("sync consumer panicked"))?
    })?;

    state.writer.lock().unwrap().as_mut().unwrap().commit()?;
    state.reader.reload()?;
    Ok(SyncStats {
        updated: res.0,
        removed: res.1,
    })
}

/// Incrementally update a single changed path (add/modify => reindex, missing => delete).
pub fn update_path(state: &State, path: &Path) {
    let enc = {
        let roots = state.roots.lock().unwrap();
        roots
            .iter()
            .find(|(prefix, _)| path.starts_with(prefix))
            .map(|(_, e)| *e)
    };
    let enc = match enc {
        Some(e) => e,
        None => return, // not under a watched root
    };
    let path_str = path.to_string_lossy().into_owned();
    let guard = state.writer.lock().unwrap();
    if let Some(w) = guard.as_ref() {
        w.delete_term(Term::from_field_text(state.fields.path, &path_str));
        if path.is_file() {
            if let Some((mt, tris)) = file_meta(path, enc) {
                let _ = w.add_document(make_doc(
                    &state.fields,
                    &path_str,
                    enc_name_of(enc),
                    mt,
                    tris,
                ));
            }
        }
    }
}

/// Replace the (possibly killed) IndexWriter with a fresh one — used to recover from
/// transient io errors (e.g. antivirus touching the index files) and retry a build.
///
/// The old writer is dropped BEFORE the new one is created. On Windows, Tantivy holds an
/// exclusive file lock (.tantivy-writer.lock) for the lifetime of the IndexWriter. writer_with_num_threads()
/// blocks indefinitely in Tantivy's blocking-lock mode if AV holds the lock file open. To avoid
/// this hang, we probe the lock file directly (exponential backoff, 30s timeout) and only call
/// writer_with_num_threads() once the file is accessible. Returns Err if still locked after 30s —
/// the caller should skip the current root and advise the user to run `indexify sync`.
pub fn recreate_writer(state: &State, tantivy_dir: &Path) -> Result<()> {
    {
        let mut guard = state.writer.lock().unwrap();
        *guard = None; // drop old writer → releases Tantivy's directory-level lock
    }

    // Probe .tantivy-writer.lock before calling writer_with_num_threads().
    // If AV has the file open with exclusive access, our probe fails immediately (no blocking).
    // We retry with exponential backoff rather than calling the blocking Tantivy API directly.
    let lock_path = tantivy_dir.join(".tantivy-writer.lock");
    let mut delay_ms = 600u64;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        std::thread::sleep(Duration::from_millis(delay_ms));
        match std::fs::OpenOptions::new().write(true).create(true).open(&lock_path) {
            Ok(_) => break, // accessible; _file dropped immediately — tiny TOCTOU window before Tantivy opens it
            Err(_) if Instant::now() < deadline => {
                eprintln!("  waiting for writer lock (antivirus may be scanning)…");
                delay_ms = (delay_ms * 2).min(5000);
            }
            Err(e) => {
                return Err(anyhow!(
                    "writer lock unavailable after 30s ({e}); \
                    exclude .indexify/tantivy from antivirus, then run `indexify sync`"
                ));
            }
        }
    }

    let w = state
        .index
        .writer_with_num_threads::<TantivyDocument>(1, WRITER_HEAP_BYTES)?;
    *state.writer.lock().unwrap() = Some(w);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- distinct_ngrams ---

    #[test]
    fn distinct_ngrams_basic() {
        // "abcd" → bigrams ab,bc,cd + trigrams abc,bcd
        let mut v = distinct_ngrams("abcd");
        v.sort();
        assert_eq!(v, vec!["ab", "abc", "bc", "bcd", "cd"]);
    }

    #[test]
    fn distinct_ngrams_deduplicates() {
        // "aaa" → bigram "aa" + trigram "aaa"
        let mut v = distinct_ngrams("aaa");
        v.sort();
        assert_eq!(v, vec!["aa", "aaa"]);
    }

    #[test]
    fn distinct_ngrams_two_chars_yields_bigram() {
        // a 2-char input now produces exactly its bigram — this is what makes 2-char search work
        assert_eq!(distinct_ngrams("ab"), vec!["ab"]);
    }

    #[test]
    fn distinct_ngrams_empty_string() {
        assert!(distinct_ngrams("").is_empty());
    }

    #[test]
    fn distinct_ngrams_single_char() {
        assert!(distinct_ngrams("a").is_empty());
    }

    #[test]
    fn distinct_ngrams_exactly_three_chars() {
        // "abc" → bigrams ab,bc + trigram abc
        let mut v = distinct_ngrams("abc");
        v.sort();
        assert_eq!(v, vec!["ab", "abc", "bc"]);
    }

    #[test]
    fn distinct_ngrams_japanese() {
        // "検索テスト" → 5 chars → 4 bigrams + 3 trigrams = 7
        assert_eq!(distinct_ngrams("検索テスト").len(), 7);
    }

    #[test]
    fn distinct_ngrams_two_char_japanese_word() {
        // a 2-char CJK word like 契約 yields its bigram, so it becomes searchable
        assert_eq!(distinct_ngrams("契約"), vec!["契約"]);
    }

    #[test]
    fn distinct_ngrams_mixed_ascii_japanese() {
        // "ab検索テ" → 5 chars → 4 bigrams + 3 trigrams = 7, char-based windows
        let v = distinct_ngrams("ab検索テ");
        assert_eq!(v.len(), 7);
        let set: std::collections::HashSet<_> = v.into_iter().collect();
        assert!(set.contains("ab"));
        assert!(set.contains("ab検"));
    }

    // --- is_binary ---

    #[test]
    fn is_binary_plain_text() {
        assert!(!is_binary(b"hello world\nthis is text\n"));
    }

    #[test]
    fn is_binary_with_null_byte() {
        let mut data = b"hello world".to_vec();
        data.push(0);
        assert!(is_binary(&data));
    }

    #[test]
    fn is_binary_empty() {
        assert!(!is_binary(b""));
    }

    #[test]
    fn is_binary_null_at_8192_boundary() {
        // null at exactly index 8192 → outside the scan window → not binary
        let mut data = vec![b'a'; 8193];
        data[8192] = 0;
        assert!(!is_binary(&data));

        // null at index 8191 → inside → binary
        let mut data2 = vec![b'a'; 8193];
        data2[8191] = 0;
        assert!(is_binary(&data2));
    }

    #[test]
    fn is_binary_null_at_first_byte() {
        let data = [0u8, b'a', b'b'];
        assert!(is_binary(&data));
    }

    // --- file_mtime_ms ---

    #[test]
    fn file_mtime_ms_real_file() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let meta = std::fs::metadata(tmp.path()).unwrap();
        let ms = file_mtime_ms(&meta);
        // any modern filesystem gives a non-zero mtime
        assert!(ms > 0);
    }
}
