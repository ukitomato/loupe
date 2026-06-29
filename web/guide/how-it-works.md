# How It Works

## Architecture overview

```
   CLI / MCP server / VS Code extension
     │  (all read settings.json + the same index)
     ▼
   indexify  (Rust / Tantivy)
     ├─ build:   parallel walk → per-file decode → bigrams+trigrams → index
     ├─ sync:    compare mtimes → reindex only changed files, drop deleted
     ├─ watch:   notify FS events → incremental update (debounced)
     └─ search:  n-gram-AND candidates → parallel verify (substring / regex)
```

All three front-ends — CLI, MCP server, VS Code extension — share the same on-disk index and read the same `settings.json`. They can never disagree about which files are indexed or how they are encoded.

---

## Index layout

The index lives at `<workspace>/.indexify/` by default (override with `--index-dir` or `$INDEXIFY_INDEX_DIR`):

```
.indexify/
├── settings.json   # committable — roots + encodings
├── meta.json       # index metadata (git-ignored)
└── tantivy/        # Tantivy segment files (git-ignored)
```

`settings.json` is designed to be committed. The index body is large and regenerable, so it is added to `.gitignore` automatically during `init`.

---

## Encoding detection

When indexify encounters a folder, it reads the encoding assignment from `settings.json`. It decodes each file's bytes to Unicode at index time using that assignment — so the Tantivy index always stores Unicode text regardless of the source encoding.

Supported encodings:

| Label | Encoding |
|---|---|
| `utf-8` | UTF-8 (default) |
| `shift_jis` | Shift_JIS / Windows-31J |
| `euc-jp` | EUC-JP |

Assign encodings per root with the `@enc` suffix in `init`:

```bash
indexify init --root src --root legacy@shift_jis --root old@euc-jp
```

---

## n-gram indexing (bigrams + trigrams)

For each file, indexify extracts all **bigrams** (2-char sequences) and **trigrams** (3-char sequences) from the decoded Unicode text, deduplicates them, and stores them in Tantivy. This allows any substring query of length ≥ 2 to be answered by looking up n-gram candidates first — without scanning file content at query time.

::: tip Why bigrams?
v0.3.0 added bigram support so that 2-character Japanese words (e.g., `契約`, `顧客`) are indexable. Before that, only queries with ≥ 3 characters could use the index for pre-filtering.
:::

---

## Search: candidate selection + verify

Search happens in two stages:

1. **Candidate selection** — The query string is split into n-grams (the AND of all n-grams must match), and Tantivy returns the set of candidate documents. For regex, the literal run(s) of ≥ 2 characters are extracted and used as the candidate filter.

2. **Parallel verify** — The candidate files are read and their decoded text is scanned line-by-line for the actual substring or regex match (using the `regex` crate). This is done in parallel via `rayon`. Only matches that survive verify are returned.

This two-stage approach (Zoekt / codesearch style) keeps the candidate set small while guaranteeing exact results.

### Case sensitivity

The n-gram phase always indexes and queries **lowercase** n-grams, so it over-approximates candidates regardless of case. The verify step then re-checks the original bytes for exact case when `--case-sensitive` is set. This means case-sensitive searches are always correct but slightly slower on large candidate sets.

### Incomplete results notice

If the candidate set exceeds the internal cap (most likely for a very short, very common query), indexify prints a notice on stderr (CLI) or flags the response (MCP / sidecar) rather than silently returning a partial result.

---

## Incremental sync

`sync` (and the auto-sync before each `search`) works by comparing the current filesystem state with what is recorded in `meta.json`:

- Files that are **new** → indexed
- Files that **changed** (mtime differs) → deleted from index, re-indexed
- Files that were **deleted** → removed from index

Only the changed subset is touched, so sync is fast even on large trees.

---

## Filesystem watching

The `serve` command (used by the VS Code extension) and `mcp` command both start a filesystem watcher (via the `notify` crate) in addition to serving requests. Changed paths are debounced and batched into incremental index updates. This keeps the index fresh without polling.

---

## Front-end communication

| Front-end | Protocol |
|---|---|
| CLI | Direct library calls (same binary) |
| VS Code extension | NDJSON over stdio (`serve` sidecar) |
| MCP clients (AI agents) | Model Context Protocol over stdio (`mcp`) |

The sidecar (`serve`) and MCP server both stream results back as newline-delimited JSON, allowing the client to display results progressively.
