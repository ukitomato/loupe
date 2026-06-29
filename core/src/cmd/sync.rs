// sync — incremental catch-up: reindex changed/new files, drop deleted ones. Reuses the roots
// recorded in settings.json (configure them with `indexify init`).

use anyhow::{bail, Result};
use std::time::Instant;

use crate::index::{builder, open_state};
use crate::store;

const MAX_ATTEMPTS: u32 = 6;

pub fn run(index_dir: Option<&str>) -> Result<()> {
    let dir = store::resolve_index_dir(index_dir);
    if !store::index_built(&dir) {
        bail!("no index at {}. Run `indexify build` first.", dir.display());
    }
    let roots = store::resolved_roots(&dir)?;
    let tdir = store::tantivy_dir(&dir);
    let state = open_state(&tdir)?;
    state.set_roots(&roots);

    let t0 = Instant::now();
    let progress = |n: u64| eprintln!("  reindexed {n} ({:.1}s)", t0.elapsed().as_secs_f64());

    let mut attempt = 1;
    let mut result = builder::sync_all(&state, progress);
    while result.is_err() && attempt < MAX_ATTEMPTS {
        attempt += 1;
        eprintln!("  retrying (attempt {attempt})…");
        let _ = builder::recreate_writer(&state, &tdir);
        result = builder::sync_all(&state, progress);
    }
    let stats = result?;
    let secs = t0.elapsed().as_secs_f64();

    let mut meta = store::load_meta(&dir);
    meta.last_sync = Some(store::now_rfc3339());
    meta.file_count = state.num_docs();
    let _ = store::save_meta(&dir, &meta);

    println!(
        "synced: {} updated, {} removed in {secs:.1}s",
        stats.updated, stats.removed
    );
    Ok(())
}
