# Larch — Local Markdown AI Knowledge Engine — Implementation Plan

A local-first, single-binary Rust CLI & daemon that manages a centralized Markdown vault, provides full-text search via tantivy (with Chinese tokenization), and exposes both REST and MCP interfaces for AI clients.

## Design Decisions & Open Questions

> [!IMPORTANT]
> **Chinese tokenizer**: The spec targets a knowledge base that almost certainly contains Chinese content. I will integrate `tantivy-jieba` to provide proper Chinese word segmentation alongside the default ASCII/Latin tokenizer.

> [!IMPORTANT]
> **Vault metadata location**: The tantivy index and config will live under `<vault_root>/.larch/` (similar to `.git/`). This keeps all state co-located with the vault and allows easy backup/migration. You should add `.larch/` to `.gitignore` if versioning the vault.

> [!NOTE]
> **MCP transport**: The spec mentions MCP but doesn't specify transport. I will implement **STDIO transport** (stdin/stdout JSON-RPC), which is the standard for local CLI-launched MCP servers (used by Claude Desktop, Cursor, etc.). This can be started with `larch mcp`.

> [!NOTE]
> **`larch serve` scope**: `larch serve` will start the file watcher + REST API server. MCP uses STDIO (a separate process), so `larch mcp` is a dedicated subcommand. This matches how real MCP tools work.

---

## Proposed Changes

### Project Structure

```
larch/
├── Cargo.toml
├── src/
│   ├── main.rs            # CLI entry point (clap)
│   ├── lib.rs              # Re-exports
│   ├── config.rs           # Vault path, .larch dir, assets dir management
│   ├── parser.rs           # Markdown → Chunks (pulldown-cmark)
│   ├── assets.rs           # Static asset import, hash-rename, path rewrite
│   ├── index.rs            # Tantivy schema, indexing, search
│   ├── watcher.rs          # File system watcher (notify)
│   ├── server.rs           # REST API (axum)
│   └── mcp.rs              # MCP server (rmcp, STDIO)
└── larch-spec-v1.0.md      # Original spec (existing)
```

---

### Dependencies — Cargo.toml

#### [NEW] [Cargo.toml](file:///c:/Users/Admin/Workspace/larch/Cargo.toml)

| Crate | Version | Purpose |
|-------|---------|---------|
| `clap` | 4 | CLI argument parsing with derive |
| `tantivy` | 0.22 | Full-text search engine |
| `tantivy-jieba` | 0.11 | Chinese tokenizer for tantivy |
| `pulldown-cmark` | 0.12 | Markdown parsing with offset iter |
| `axum` | 0.8 | HTTP server & routing |
| `tokio` | 1 | Async runtime |
| `notify` | 8 | File system event watcher |
| `notify-debouncer-full` | 0.5 | Event debouncing |
| `serde` / `serde_json` | 1 | Serialization |
| `anyhow` | 1 | Error handling |
| `rmcp` | latest | MCP protocol (STDIO server) |
| `gray_matter` | 0.2 | YAML frontmatter extraction |
| `regex` | 1 | Inline `#tag` extraction |
| `sha2` | 0.10 | Content hashing for asset dedup/rename |
| `chrono` | 0.4 | Timestamps for logs |

> [!NOTE]
> I'll pin to compatible ranges (e.g., `tantivy = "0.22"`) and let cargo resolve exact versions. Using tantivy 0.22 for stable `tantivy-jieba` compatibility.

---

### Core Config

#### [NEW] [config.rs](file:///c:/Users/Admin/Workspace/larch/src/config.rs)

- `VaultConfig` struct: holds `vault_root: PathBuf`
- `larch_dir()` → `vault_root/.larch/`
- `index_dir()` → `vault_root/.larch/index/`
- `logs_dir()` → `vault_root/.larch/logs/`
- `assets_dir()` → `vault_root/assets/` (global static asset storage)
- Init logic: create `.larch/`, `.larch/index/`, `.larch/logs/`, `assets/` on `larch init`
- Helper: resolve absolute path, validate directory exists

---

### Markdown Parser & Chunker

#### [NEW] [parser.rs](file:///c:/Users/Admin/Workspace/larch/src/parser.rs)

Core structs:
```rust
/// File-level metadata from YAML frontmatter
pub struct FileMeta {
    pub title: Option<String>,
    pub date: Option<String>,
    pub tags: Vec<String>,         // explicit tags from YAML
    pub summary: Option<String>,
    pub version: Option<String>,
}

pub struct Chunk {
    pub chunk_id: String,          // hash_startline
    pub file_path: String,
    pub title_hierarchy: String,   // "Architecture > DB > SQLite"
    pub content: String,
    pub start_line: u64,
    pub end_line: u64,
    pub keywords: Vec<String>,     // merged: YAML tags + inline #tags
}
```

Algorithm:
1. **Frontmatter extraction** — Use `gray_matter::Matter::parse(source)` to split YAML header from body. Deserialize YAML into `FileMeta` (title, date, tags, summary, version). The remaining body goes to step 2.
2. Pre-compute byte-offset → line-number lookup table (scan for `\n`, accounting for frontmatter line offset)
3. Use `pulldown_cmark::Parser::new_ext(body, Options::all()).into_offset_iter()`
4. Walk events; on `Event::Start(Tag::Heading { level, .. })`, start a new chunk scope
5. Accumulate text events into current chunk's `content`
6. On next heading of same-or-higher level, finalize previous chunk with `end_line`
7. Maintain a stack for title hierarchy (push on deeper heading, pop on same/higher)
8. Content before any heading → a "preamble" chunk (title = `FileMeta.title` or filename)
9. **Inline tag extraction** — After each chunk is finalized, scan `content` with regex `r"#([^\s#]+)"` to extract inline hashtags (e.g., `#性能优化`). Merge with `FileMeta.tags`, deduplicate, and store in `chunk.keywords`.

---

### Static Asset Handling

#### [NEW] [assets.rs](file:///c:/Users/Admin/Workspace/larch/src/assets.rs)

All images, PDFs, and other non-[.md](file:///c:/Users/Admin/Workspace/larch/spec-meta.md) files referenced by Markdown are treated as static **assets**. They are co-located in a global `<vault_root>/assets/` directory to ensure portability (Git-safe, cross-machine).

**Import pipeline** (called during `larch import` and watcher events):
1. **Scan links** — Use `pulldown-cmark` events (`Event::Start(Tag::Image { .. })`, `Event::Start(Tag::Link { .. })`) or regex to find all `![alt](path)` and `[text](path)` references in the markdown.
2. **Copy & hash-rename** — For each local-path reference:
   - Read the file, compute SHA256 → take first 8 hex chars.
   - Copy to `<vault_root>/assets/<prefix>_<hash8>.<ext>` (e.g., `img_a1b2c3d4.png`). If the target already exists (same hash = same content), skip copy (dedup).
3. **Rewrite markdown** — Replace original paths in the [.md](file:///c:/Users/Admin/Workspace/larch/spec-meta.md) source with relative paths to `assets/` (e.g., `../assets/img_a1b2c3d4.png`), then save the rewritten [.md](file:///c:/Users/Admin/Workspace/larch/spec-meta.md) to vault.
4. **Alt-text indexing** — The alt text (e.g., `核心架构图`) is preserved in the chunk `content` by pulldown-cmark and will be indexed by Tantivy. Image files themselves are NOT indexed.

Key functions:
- `process_assets(md_content, source_dir, vault_root) → String` — returns rewritten markdown content
- `hash_file(path) → String` — SHA256-based short hash

---

### Tantivy Index Engine

#### [NEW] [index.rs](file:///c:/Users/Admin/Workspace/larch/src/index.rs)

Schema fields (matching spec §4 + metadata spec):
- `file_path`: STRING, indexed + stored
- `chunk_id`: STRING, indexed + stored  
- `title_hierarchy`: TEXT, indexed + stored
- `content`: TEXT, indexed + stored (with jieba tokenizer, includes image alt-text)
- `start_line`: u64, indexed + stored
- `end_line`: u64, indexed + stored
- `keywords`: TEXT, indexed + stored (merged explicit + inline tags)
- `summary`: TEXT, stored (from YAML frontmatter, for preview display)
- `version`: STRING, stored (document version from YAML)
- `created_at`: STRING, stored + indexed (date from YAML, for sorting/filtering)

Key functions:
- `open_or_create(index_dir) → Index` — register jieba tokenizer, open/create index
- `index_file(writer, file_path, chunks, meta)` — delete old docs for file, add new chunks with metadata
- `remove_file(writer, file_path)` — delete all chunks for a file
- `search(index, query_str, limit) → Vec<SearchResult>` — parse query, execute, return results with snippets
- `SearchResult` struct: `chunk_id, file_path, title_hierarchy, content_snippet, start_line, end_line, score`

---

### File Watcher

#### [NEW] [watcher.rs](file:///c:/Users/Admin/Workspace/larch/src/watcher.rs)

- Use `notify_debouncer_full::new_debouncer` with ~500ms timeout
- Watch vault_root recursively, filter for `*.md` files
- On debounced events:
  - **Create/Modify** → re-parse & re-index the file
  - **Remove** → delete chunks from index
  - **Rename** → remove old + index new
- Runs in a background tokio task, receives events via channel
- Emits structured log entries to `.larch/logs/` for `larch logs` consumption

---

### CLI (clap)

#### [NEW] [main.rs](file:///c:/Users/Admin/Workspace/larch/src/main.rs)

```
larch init            # Initialize vault (create .larch/, assets/, config). NO indexing.
larch serve [--port 3000]   # Start watcher + REST API
larch search "<query>"      # Search and print results
larch import <path>         # Import file OR directory into vault
    -x                      # Move instead of copy
    -d <dir>                # Sub-directory within vault
larch status                # Structured vault state (file count, index count, service status)
larch logs [-f]             # Print recent logs (vault changes, server, mcp). -f for real-time tail.
larch mcp                   # Start MCP server (STDIO)
```

- `init`: create `.larch/`, `.larch/index/`, `.larch/logs/`, `assets/` directories + write initial config. **No indexing** — use `import` to add content.
- `serve`: spawn watcher task + axum HTTP server, log activity to `.larch/logs/`
- `search`: open index read-only, query, print colored results with line numbers
- `import`: accepts **single file or directory** (recursive scan for `*.md`). For each [.md](file:///c:/Users/Admin/Workspace/larch/spec-meta.md): process assets → copy/move → parse → index. Non-[.md](file:///c:/Users/Admin/Workspace/larch/spec-meta.md) files in a directory import are skipped.
- `status`: read vault state and print structured summary — total [.md](file:///c:/Users/Admin/Workspace/larch/spec-meta.md) files, indexed chunk count, `serve` running (check PID file), MCP status.
- `logs`: read and print recent log entries from `.larch/logs/`, categorized (vault changes / server / mcp). With `-f` flag, tail logs in real-time (like `tail -f`).
- `mcp`: launch STDIO MCP server

---

### REST API

#### [NEW] [server.rs](file:///c:/Users/Admin/Workspace/larch/src/server.rs)

| Route | Method | Description |
|-------|--------|-------------|
| `/api/v1/search?q=<kw>&limit=10` | GET | Full-text search, returns JSON array of `SearchResult` |
| `/api/v1/document?path=<fp>&start_line=x&end_line=y` | GET | Read lines from source file |
| `/api/v1/import` | POST | Accept `{ content, filename, dir? }`, write to vault |
| `/health` | GET | Health check / status |

Shared state via `axum::extract::State(Arc<AppState>)` holding `Index` reader + `VaultConfig`.

---

### MCP Server

#### [NEW] [mcp.rs](file:///c:/Users/Admin/Workspace/larch/src/mcp.rs)

Tools (via `rmcp` derive macros):
- `search_local_knowledge(query: String, limit: Option<u32>)` → search results JSON
- `read_file_context(file_path: String, start_line: u64, end_line: u64)` → file content

Transport: STDIO (stdin/stdout JSON-RPC 2.0), launched via `larch mcp`.

---

### Lib Re-exports

#### [NEW] [lib.rs](file:///c:/Users/Admin/Workspace/larch/src/lib.rs)

Re-export all modules for clean internal access.

---

## Verification Plan

### Automated Tests

1. **Parser unit tests** (`cargo test`):
   - Parse a sample [.md](file:///c:/Users/Admin/Workspace/larch/spec-meta.md) with multiple heading levels → verify chunk count, line numbers, title hierarchy
   - Parse a file with no headings → single preamble chunk
   - Parse a file with CJK content → content extracted correctly
   - Parse a file with YAML frontmatter → verify `FileMeta` fields (title, tags, summary, version)
   - Verify inline `#tag` extraction and dedup merging with YAML tags

2. **Asset unit tests** (`cargo test`):
   - Markdown with `![alt](local/image.png)` → asset copied to `assets/`, path rewritten
   - Duplicate asset (same content hash) → no duplicate copy
   - Alt text preserved in chunk content for indexing

3. **Index unit tests** (`cargo test`):
   - Create index in temp dir, index sample chunks, search → verify results
   - Index file, delete file, search → no results
   - Chinese content search → jieba tokenizer returns matches

4. **Build verification**: `cargo build --release` succeeds with no errors

5. **Full test suite**: `cargo test` — all pass

### Manual Verification

After building, test end-to-end:

```powershell
# 1. Build
cargo build

# 2. Initialize vault (creates dirs only, no indexing)
./target/debug/larch init ./test_vault

# 3. Import files (single file + directory)
./target/debug/larch import ./some_file.md -d notes/daily
./target/debug/larch import ./docs_folder/ -d imported

# 4. Check vault state
./target/debug/larch status

# 5. Search
./target/debug/larch search "keyword"

# 6. Start server, then curl
./target/debug/larch serve --port 3000
curl "http://localhost:3000/api/v1/search?q=keyword&limit=5"
curl "http://localhost:3000/health"

# 7. View logs
./target/debug/larch logs
./target/debug/larch logs -f
```
