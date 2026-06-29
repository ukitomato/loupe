// search — query the index from the CLI. Human-readable `path:line: text` by default, or a JSON
// array with --json. Prints a clear, structured error if the index hasn't been built yet.
//
// Freshness is automatic: before searching we run a quiet incremental sync (mtime-based catch-up)
// so the CLI/skill caller never has to think about staleness. `--no-sync` skips it for raw speed.

use anyhow::Result;
use std::path::Path;

use crate::index::{builder, open_state, searcher, State};
use crate::store;

const SYNC_MAX_ATTEMPTS: u32 = 6;

pub fn run(
    index_dir: Option<&str>,
    query: &str,
    regex: bool,
    max: usize,
    json: bool,
    no_sync: bool,
    case_sensitive: bool,
) -> Result<()> {
    let dir = store::resolve_index_dir(index_dir);

    if !store::index_built(&dir) {
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "error": "index_not_found",
                    "message": "Run `indexify build` first.",
                    "index_dir": dir.display().to_string(),
                })
            );
        } else {
            eprintln!("no index at {}. Run `indexify build` first.", dir.display());
        }
        return Ok(());
    }

    let state = open_state(&store::tantivy_dir(&dir))?;

    if !no_sync {
        auto_sync(&dir, &state);
    }

    let outcome = searcher::search(&state, query, regex, max, case_sensitive)?;
    let hits = &outcome.hits;

    if json {
        let arr: Vec<_> = hits
            .iter()
            .map(|h| serde_json::json!({ "file": h.file, "line": h.line, "text": h.text }))
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr)?);
    } else {
        let cwd = std::env::current_dir().unwrap_or_default();
        for h in hits {
            println!("{}:{}: {}", relativize(&cwd, &h.file), h.line, h.text);
        }
        eprintln!("{} results", hits.len());
    }
    if outcome.candidates_truncated {
        eprintln!(
            "note: candidate limit reached — results may be incomplete; narrow the query (longer or more specific) for full coverage."
        );
    }
    Ok(())
}

/// Best-effort incremental catch-up before a search. Silent on the happy path; never fails the
/// search (a stale-but-usable index beats no results), only notes problems on stderr.
fn auto_sync(dir: &Path, state: &State) {
    let roots = match store::resolved_roots(dir) {
        Ok(r) if !r.is_empty() => r,
        _ => return, // no configured roots → nothing to sync against
    };
    state.set_roots(&roots);

    let tdir = store::tantivy_dir(dir);
    let mut attempt = 1;
    let mut result = builder::sync_all(state, |_| {});
    while result.is_err() && attempt < SYNC_MAX_ATTEMPTS {
        attempt += 1;
        let _ = builder::recreate_writer(state, &tdir);
        result = builder::sync_all(state, |_| {});
    }
    match result {
        Ok(stats) if stats.updated > 0 || stats.removed > 0 => {
            let mut meta = store::load_meta(dir);
            meta.last_sync = Some(store::now_rfc3339());
            meta.file_count = state.num_docs();
            let _ = store::save_meta(dir, &meta);
        }
        Ok(_) => {}
        Err(e) => eprintln!("auto-sync skipped ({e}); results may be stale"),
    }
}

/// Show paths relative to the current directory when possible; fall back to the absolute path.
fn relativize(cwd: &Path, file: &str) -> String {
    Path::new(file)
        .strip_prefix(cwd)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| file.to_string())
}
