// index — the live Tantivy index (schema, building/syncing, searching).

pub mod builder;
pub mod schema;
pub mod searcher;

pub use schema::{build_schema, Fields};

use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tantivy::tokenizer::{NgramTokenizer, TextAnalyzer};
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument};

pub const WRITER_HEAP_BYTES: usize = 100_000_000;

/// Everything needed to build/search the index, shared across threads.
pub struct State {
    pub index: Index,
    pub writer: Mutex<Option<IndexWriter>>,
    pub reader: IndexReader,
    pub fields: Fields,
    /// (root prefix, encoding) pairs, used by sync and the watcher to decode changed files.
    pub roots: Mutex<Vec<(PathBuf, &'static encoding_rs::Encoding)>>,
}

/// Open (or create) the Tantivy index living in `tantivy_dir`.
pub fn open_state(tantivy_dir: &Path) -> Result<Arc<State>> {
    let (schema, fields) = build_schema();
    std::fs::create_dir_all(tantivy_dir)?;
    let dir = tantivy::directory::MmapDirectory::open(tantivy_dir)?;
    let index = Index::open_or_create(dir, schema)?;
    let analyzer = TextAnalyzer::builder(NgramTokenizer::new(3, 3, false)?).build();
    index.tokenizers().register("tri", analyzer);
    // Single writer thread = segment files written serially → minimal concurrent file I/O,
    // which sharply reduces transient io errors when antivirus scans the index on Windows.
    let writer = index.writer_with_num_threads::<TantivyDocument>(1, WRITER_HEAP_BYTES)?;
    let reader = index
        .reader_builder()
        .reload_policy(ReloadPolicy::OnCommitWithDelay)
        .try_into()?;
    Ok(Arc::new(State {
        index,
        writer: Mutex::new(Some(writer)),
        reader,
        fields,
        roots: Mutex::new(Vec::new()),
    }))
}

impl State {
    /// Replace the recorded roots (used by sync and the watcher). `roots` is (path, encoding-name).
    pub fn set_roots(&self, roots: &[(PathBuf, String)]) {
        let mut g = self.roots.lock().unwrap();
        g.clear();
        for (p, enc) in roots {
            g.push((p.clone(), crate::encoding::enc_by_name(enc)));
        }
    }

    /// Number of live documents (= indexed files) currently visible to searches.
    pub fn num_docs(&self) -> u64 {
        self.reader.searcher().num_docs()
    }
}
