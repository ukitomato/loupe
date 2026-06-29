// build — full index (re)build from the roots in settings.json. Configure those roots with
// `indexify init` (interactive or via --root); build itself takes no roots.

use anyhow::{bail, Result};
use std::time::Instant;

use crate::index::{builder, open_state};
use crate::store;

const MAX_ATTEMPTS: u32 = 6;

pub fn run(index_dir: Option<&str>, force: bool) -> Result<()> {
    let dir = store::resolve_index_dir(index_dir);

    let resolved = match store::resolved_roots(&dir) {
        Ok(r) => r,
        Err(_) => bail!(
            "no roots configured in {}.\n  run `indexify init` first (interactive), or e.g. `indexify init --root src --root legacy@shift_jis`",
            store::settings_path(&dir).display()
        ),
    };
    store::ensure_gitignore(&dir)?;

    let tdir = store::tantivy_dir(&dir);
    if force && tdir.exists() {
        std::fs::remove_dir_all(&tdir)?;
    }
    let state = open_state(&tdir)?;
    state.set_roots(&resolved);

    let t0 = Instant::now();
    let mut total = 0u64;
    for (abs, enc) in &resolved {
        let root_str = abs.to_string_lossy().into_owned();
        eprintln!("indexing {root_str} ({enc})…");
        total += build_one(&state, &root_str, enc, &t0)?;
    }
    let secs = t0.elapsed().as_secs_f64();

    let mut meta = store::load_meta(&dir);
    meta.last_build = Some(store::now_rfc3339());
    meta.file_count = state.num_docs();
    let _ = store::save_meta(&dir, &meta);

    let size = store::index_size_bytes(&dir);
    println!(
        "built {total} files in {secs:.1}s — index {:.1} MB at {}",
        size as f64 / 1_048_576.0,
        tdir.display()
    );
    Ok(())
}

/// Build a single root, retrying with a fresh writer on transient io errors.
fn build_one(state: &crate::index::State, root: &str, enc: &str, t0: &Instant) -> Result<u64> {
    let mut attempt = 0;
    loop {
        attempt += 1;
        let progress = |n: u64| eprintln!("  indexed {n} ({:.1}s)", t0.elapsed().as_secs_f64());
        match builder::build_root(state, root, enc, progress) {
            Ok(n) => return Ok(n),
            Err(e) if attempt < MAX_ATTEMPTS => {
                eprintln!("  retrying (attempt {attempt}) after error: {e}");
                builder::recreate_writer(state)?;
            }
            Err(e) => return Err(e),
        }
    }
}
