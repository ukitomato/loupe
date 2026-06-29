// searcher.rs — n-gram-AND candidate retrieval, then parallel line-level verification.
//
//   substring: lowercased needle (>=2 chars) -> distinct n-grams (the bigram for a 2-char needle,
//              trigrams otherwise) -> candidate docs -> memmem on an ascii-lowercased copy of each
//              candidate file
//   regex:     literal runs (>=2 chars) of the pattern give the n-grams; the full regex
//              (case-insensitive) verifies each line

use anyhow::{anyhow, Result};
use rayon::prelude::*;
use std::collections::HashSet;
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, Occur, Query, TermQuery};
use tantivy::schema::{IndexRecordOption, Value};
use tantivy::{TantivyDocument, Term};

use super::State;
use crate::encoding::enc_by_name;

const CANDIDATE_LIMIT: usize = 2000;
const PER_FILE_MATCH_CAP: usize = 50;

#[derive(Clone)]
pub struct Hit {
    pub file: String,
    pub line: usize,
    pub text: String,
}

/// Result of a search: the hits plus whether the candidate set reached `CANDIDATE_LIMIT` (so some
/// matching files may not have been verified — results can be incomplete and the caller should
/// suggest a narrower query). Most likely for a very common short query, e.g. a 2-char query whose
/// bigram occurs in a huge number of files.
#[derive(Clone, Default)]
pub struct SearchOutcome {
    pub hits: Vec<Hit>,
    pub candidates_truncated: bool,
}

/// Extract lowercased literal runs from a regex pattern: maximal runs of ASCII [A-Za-z0-9_], or any
/// non-ASCII char (CJK etc. are never regex metacharacters, so they are literal), of length >= 2
/// counted in *chars* (not bytes — a CJK char is one char but several bytes). Such a run is
/// necessarily contiguous in any match, so its n-grams (bigram for a 2-char run, trigrams for
/// longer) can pre-filter candidates.
fn extract_literals(pattern: &str) -> Vec<String> {
    fn flush(cur: &mut String, runs: &mut Vec<String>) {
        if cur.chars().count() >= 2 {
            runs.push(std::mem::take(cur));
        } else {
            cur.clear();
        }
    }
    let mut runs = Vec::new();
    let mut cur = String::new();
    for c in pattern.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            cur.push(c.to_ascii_lowercase());
        } else if !c.is_ascii() {
            cur.push(c);
        } else {
            flush(&mut cur, &mut runs);
        }
    }
    flush(&mut cur, &mut runs);
    runs
}

/// Collect the distinct candidate-filter n-grams for a set of literal strings: a 2-char literal
/// contributes its single bigram, a literal of >=3 chars contributes its (more selective)
/// trigrams, and 0/1-char literals contribute nothing. These lengths match what `distinct_ngrams`
/// stores at index time, so the terms exist in the index.
fn ngrams_of<'a>(strs: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut seen = HashSet::new();
    for s in strs {
        let chars: Vec<char> = s.chars().collect();
        if chars.len() == 2 {
            seen.insert(chars.iter().collect::<String>());
        } else {
            for w in chars.windows(3) {
                seen.insert(w.iter().collect::<String>());
            }
        }
    }
    seen.into_iter().collect()
}

fn line_starts(bytes: &[u8]) -> Vec<usize> {
    let mut v = vec![0usize];
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\n' {
            v.push(i + 1);
        }
    }
    v
}

// Box the large Finder variant to even out the enum size.
enum Verifier {
    Substr {
        finder: Box<memchr::memmem::Finder<'static>>,
        case_sensitive: bool,
    },
    Regex(regex::Regex),
}

pub fn search(
    state: &State,
    query: &str,
    regex_mode: bool,
    max: usize,
    case_sensitive: bool,
) -> Result<SearchOutcome> {
    let searcher = state.reader.searcher();

    // Build the trigram candidate set + the line verifier.
    let (grams, verifier): (Vec<String>, Verifier) = if regex_mode {
        let runs = extract_literals(query);
        if runs.is_empty() {
            return Err(anyhow!(
                "regex needs a literal substring of >=2 chars to use the index"
            ));
        }
        let re = if case_sensitive {
            regex::Regex::new(query)?
        } else {
            regex::Regex::new(&format!("(?i){query}"))?
        };
        (
            ngrams_of(runs.iter().map(|s| s.as_str())),
            Verifier::Regex(re),
        )
    } else {
        let needle = query.to_ascii_lowercase();
        if needle.chars().count() < 2 {
            return Ok(SearchOutcome::default());
        }
        let grams = ngrams_of(std::iter::once(needle.as_str()));
        // For case-sensitive verify, the finder contains original-case bytes; for
        // case-insensitive it contains the lowercased needle (matching the lowercased haystack).
        let finder_needle: &[u8] = if case_sensitive {
            query.as_bytes()
        } else {
            needle.as_bytes()
        };
        let finder = Box::new(memchr::memmem::Finder::new(finder_needle).into_owned());
        (
            grams,
            Verifier::Substr {
                finder,
                case_sensitive,
            },
        )
    };

    let mut subs: Vec<(Occur, Box<dyn Query>)> = Vec::new();
    for tg in &grams {
        subs.push((
            Occur::Must,
            Box::new(TermQuery::new(
                Term::from_field_text(state.fields.tri, tg),
                IndexRecordOption::Basic,
            )),
        ));
    }
    let top = searcher.search(
        &BooleanQuery::new(subs),
        &TopDocs::with_limit(CANDIDATE_LIMIT),
    )?;
    // The candidate set is capped: if we hit the cap, some matching files were never verified,
    // so the result may be incomplete. Surface this so callers can suggest a narrower query.
    let candidates_truncated = top.len() >= CANDIDATE_LIMIT;

    let mut targets: Vec<(String, &'static encoding_rs::Encoding)> = Vec::with_capacity(top.len());
    for (_s, addr) in top {
        let d: TantivyDocument = searcher.doc(addr)?;
        let path = d
            .get_first(state.fields.path)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let enc_name = d
            .get_first(state.fields.enc)
            .and_then(|v| v.as_str())
            .unwrap_or("utf-8");
        targets.push((path, enc_by_name(enc_name)));
    }

    let mut hits: Vec<Hit> = targets
        .par_iter()
        .flat_map_iter(|(path, enc)| {
            let mut out = Vec::new();
            if let Ok(bytes) = std::fs::read(path) {
                let (text, _, _) = enc.decode(&bytes);
                match &verifier {
                    Verifier::Substr {
                        finder,
                        case_sensitive,
                    } => {
                        let orig = text.as_bytes();
                        let haystack_buf;
                        let haystack: &[u8] = if *case_sensitive {
                            orig
                        } else {
                            haystack_buf = text.to_ascii_lowercase().into_bytes();
                            &haystack_buf
                        };
                        let starts = line_starts(orig);
                        let mut last = usize::MAX;
                        for off in finder.find_iter(haystack) {
                            let li = match starts.binary_search(&off) {
                                Ok(i) => i,
                                Err(i) => i - 1,
                            };
                            if li == last {
                                continue;
                            }
                            last = li;
                            let s = starts[li];
                            let e = starts.get(li + 1).copied().unwrap_or(orig.len());
                            let line = String::from_utf8_lossy(&orig[s..e]).trim_end().to_string();
                            out.push(Hit {
                                file: path.clone(),
                                line: li + 1,
                                text: line,
                            });
                            if out.len() >= PER_FILE_MATCH_CAP {
                                break;
                            }
                        }
                    }
                    Verifier::Regex(re) => {
                        for (i, line) in text.lines().enumerate() {
                            if re.is_match(line) {
                                out.push(Hit {
                                    file: path.clone(),
                                    line: i + 1,
                                    text: line.trim_end().to_string(),
                                });
                                if out.len() >= PER_FILE_MATCH_CAP {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            out
        })
        .collect();
    hits.truncate(max);
    Ok(SearchOutcome {
        hits,
        candidates_truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- extract_literals ---

    #[test]
    fn extract_literals_simple() {
        assert_eq!(extract_literals("foobar"), vec!["foobar"]);
    }

    #[test]
    fn extract_literals_two_runs() {
        let mut v = extract_literals("foo.*bar");
        v.sort();
        assert_eq!(v, vec!["bar", "foo"]);
    }

    #[test]
    fn extract_literals_underscore_included() {
        assert_eq!(extract_literals("foo_bar"), vec!["foo_bar"]);
    }

    #[test]
    fn extract_literals_one_char_runs_excluded() {
        // single-char runs can't be pre-filtered → dropped
        assert!(extract_literals("a.b").is_empty());
        assert!(extract_literals("x+y").is_empty());
    }

    #[test]
    fn extract_literals_two_char_runs_included() {
        // 2-char runs are now usable (their bigram pre-filters), unlike before
        let mut v = extract_literals("ab.cd");
        v.sort();
        assert_eq!(v, vec!["ab", "cd"]);
    }

    #[test]
    fn extract_literals_cjk_run() {
        // non-ASCII chars are literal (never regex metacharacters); a 2-char CJK run is kept
        assert_eq!(extract_literals("契約.*者"), vec!["契約"]);
    }

    #[test]
    fn extract_literals_cjk_single_char_excluded() {
        // a lone CJK char (1 char) can't be pre-filtered → dropped
        assert!(extract_literals("a.者.b").is_empty());
    }

    #[test]
    fn extract_literals_pure_metachar_pattern_empty() {
        assert!(extract_literals(r"\d{3}").is_empty());
        assert!(extract_literals(r"[A-Z]+").is_empty());
        assert!(extract_literals(r"\s+").is_empty());
    }

    #[test]
    fn extract_literals_mixed_long_and_short() {
        // "foo\d+bar" → ["foo", "bar"]
        let mut v = extract_literals(r"foo\d+bar");
        v.sort();
        assert_eq!(v, vec!["bar", "foo"]);
    }

    #[test]
    fn extract_literals_trailing_run() {
        // trailing run that reaches end of string without a separator
        assert_eq!(extract_literals("abc"), vec!["abc"]);
    }

    #[test]
    fn extract_literals_lowercased() {
        let v = extract_literals("FooBar");
        assert_eq!(v, vec!["foobar"]);
    }

    // --- ngrams_of ---

    #[test]
    fn ngrams_of_basic() {
        // a >=3-char literal contributes trigrams only (more selective than bigrams)
        let g: std::collections::HashSet<_> =
            ngrams_of(std::iter::once("abcd")).into_iter().collect();
        assert!(g.contains("abc"));
        assert!(g.contains("bcd"));
        assert_eq!(g.len(), 2);
    }

    #[test]
    fn ngrams_of_two_char_literal_is_bigram() {
        // a 2-char literal contributes exactly its bigram
        assert_eq!(ngrams_of(std::iter::once("ab")), vec!["ab"]);
    }

    #[test]
    fn ngrams_of_two_char_cjk_literal_is_bigram() {
        assert_eq!(ngrams_of(std::iter::once("契約")), vec!["契約"]);
    }

    #[test]
    fn ngrams_of_one_char_literal_empty() {
        // 0/1-char literals can't be pre-filtered
        assert!(ngrams_of(["a", ""].iter().copied()).is_empty());
    }

    #[test]
    fn ngrams_of_mixed_lengths_across_strings() {
        // a 2-char run gives a bigram; a >=3-char run gives trigrams; unioned
        let g: std::collections::HashSet<_> = ngrams_of(["ab", "wxyz"].iter().copied())
            .into_iter()
            .collect();
        assert!(g.contains("ab"));
        assert!(g.contains("wxy"));
        assert!(g.contains("xyz"));
        assert_eq!(g.len(), 3);
    }

    #[test]
    fn ngrams_of_multibyte_chars() {
        // "検索テスト" (5 chars) → trigrams only (>=3) → 3
        let v = ngrams_of(std::iter::once("検索テスト"));
        assert_eq!(v.len(), 3);
    }

    // --- line_starts ---

    #[test]
    fn line_starts_empty() {
        assert_eq!(line_starts(b""), vec![0]);
    }

    #[test]
    fn line_starts_no_newline() {
        assert_eq!(line_starts(b"hello"), vec![0]);
    }

    #[test]
    fn line_starts_single_newline() {
        assert_eq!(line_starts(b"hello\nworld"), vec![0, 6]);
    }

    #[test]
    fn line_starts_trailing_newline() {
        assert_eq!(line_starts(b"hello\n"), vec![0, 6]);
    }

    #[test]
    fn line_starts_crlf() {
        // \r\n: the \n at index 6 → next line starts at 7
        assert_eq!(line_starts(b"hello\r\nworld"), vec![0, 7]);
    }

    #[test]
    fn line_starts_consecutive_newlines() {
        assert_eq!(line_starts(b"a\n\nb"), vec![0, 2, 3]);
    }

    #[test]
    fn line_starts_multiple_lines() {
        assert_eq!(line_starts(b"a\nb\nc"), vec![0, 2, 4]);
    }
}
