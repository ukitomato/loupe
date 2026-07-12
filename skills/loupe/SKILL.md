---
name: loupe
description: Full-text code search with the loupe CLI — fast substring and regular-expression search across a whole repository using a Tantivy n-gram (bigram+trigram) index. Handles mixed encodings (UTF-8, Shift_JIS, EUC-JP) in a single index. Use whenever you need to find where a string, identifier, constant, column name, or function appears across the codebase, run a repo-wide regex grep, or search source files that aren't UTF-8. The index auto-syncs before every search, so results always reflect the latest files. Prefer this for any full-text text search over a large or multi-encoding codebase.
when_to_use: |
  - Finding everywhere a string / identifier / constant / column / symbol appears across the repo
  - Running a repository-wide regular-expression search
  - Searching source files that are not UTF-8 (Shift_JIS, EUC-JP) without mojibake
  - Working in a large codebase where a plain recursive grep is too slow
argument-hint: "[text or regex to search for]"
allowed-tools: Bash
---

## MCP ツールが使える場合

`search_code` または `search_regex` が利用可能なら、それを呼び出して結果を返す。以上。

---

## CLI（MCP なしの場合）

次の1コマンドを実行し、結果をそのまま返す。

```bash
"${LOUPE_BIN:-loupe}" search "$ARGUMENTS" ${LOUPE_INDEX_DIR:+--index-dir "$LOUPE_INDEX_DIR"}
```

- 結果が多すぎる場合のみ `--max 50` を追加する
- 正規表現で検索したい場合は `--regex` を追加する

---

## エラーが出た場合のみ参照

| エラー | 対処 |
|---|---|
| `command not found: loupe` | `LOUPE_BIN` に実行ファイルのパスをセットする、またはユーザーに確認する |
| `no index` / `index_not_found` | ユーザーに `loupe build` の実行を依頼する |
| `LockBusy`（旧版） | v0.4.0 以降は自動で read-only にフォールバックするためこのエラーは出ない |
