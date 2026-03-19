# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.2] - 2026-03-19

### Added
- **Thin Client Architecture**: `larch serve` is now the single writer — MCP and CLI write commands delegate to it, eliminating concurrent IndexWriter conflicts.
- **Embedded MCP Server**: `larch serve --mcp` runs both REST API and MCP stdio server in a single process with a shared write lock.
- **CLI HTTP Delegation**: `larch import` and `larch reindex` automatically detect a running serve (via lock file) and delegate through HTTP API instead of writing directly.
- **New REST Endpoints**: `POST /api/v1/reindex` and `POST /api/v1/import/file` for server-side reindex and disk-based file import.
- **Lock File Management**: Serve writes a lock file (`.larch/serve.lock`) with PID and port; stale locks are auto-detected and cleaned up.
- **Release Build Script**: `scripts/build-release.sh` for one-command cross-compilation (macOS arm64, Linux x86_64/arm64).

### Changed
- **Search Weight Tuning**: `summary` field is now jieba-indexed and participates in search scoring. New boosts: `keywords` 2.5, `title_hierarchy` 2.0, `summary` 1.5, `content` 1.0.
- **TLS Backend**: Switched `reqwest` from native OpenSSL to `rustls` for easier cross-compilation and zero system TLS dependency.
- Standalone `larch mcp` now warns when serve is already running to prevent write conflicts.
- Graceful shutdown with Ctrl-C and automatic lock file cleanup.

### Breaking
- Schema change: `summary` field indexing requires `larch reindex` after upgrade.

## [0.2.1] - 2026-03-17

### Added
- **Singleton IndexReader**: Use a single reusable reader instead of creating one per search call, fixing empty results with small limits.
- **MCP Import Tool**: New `import` tool mirroring REST API `POST /api/v1/import`.
- **Configurable Vault Path**: `~/.larch/config.json` for custom vault locations; `larch init <path>` accepts an optional path argument.
- **Path Traversal Protection**: Import endpoints validate paths stay within vault boundary.

### Changed
- Strip ANSI codes from API/MCP responses, use `<b>` HTML tags instead.
- Pass pre-opened index to MCP server to avoid duplicate index opening.
- Auto-migrate old `~/.larch` vault layout to new config scheme.

## [0.2.0] - 2026-03-16

### Added
- **Vault Directory Tree (`tree`)**: New CLI command `larch tree` to visualize the entire vault hierarchy as a graphical tree. Also available via MCP (`tree` tool) and REST API (`GET /api/v1/tree`), supporting JSON output (`--json`).
- **Tag Aggregation (`tag ls`)**: New CLI command `larch tag ls [TAG]` to list all indexed tags and their associated documents, or filter by a specific tag. Available via MCP (`tags` tool) and REST API (`GET /api/v1/tags`).
- **Advanced Search Filters**: The `search` command now supports `--tag` and `--dir` arguments to narrow down search scopes to specific tags or subdirectories. Fully supported in the REST API and MCP server.

### Changed
- **Index Schema Optimization**: Introduced two new indexing fields: `tags` (Multivalued Fast String) and `dirs` (Facet). `keywords` is now purely reserved for explicit YAML Frontmatter meta definitions.
- **Search Weighting Strategy**: Adjusted search boosting to favor metadata over raw text: `keywords` (3.0) > `title_hierarchy` (2.0) > `content` (1.0).
- **Search Snippets (Highlighting)**: Upgraded the search result preview from basic truncation to dynamic BM25 context snippets (Tantivy `SnippetGenerator`), elegantly highlighting matching terms in terminal output.
- **Tag Reading Performance**: `tag ls` queries now directly traverse the high-speed Columnar/Fast Fields (`TermDictionary`) rather than full document deserialization, vastly improving speed on large vaults.

### Fixed
- Fixed an edge-case in Markdown parsing where the first characters of the document (e.g., `# ` from a heading) could be inadvertently stripped if the file lacked YAML frontmatter.
- Escaped HTML entities (`&lt;`, `&gt;`, `&amp;`, etc.) are now correctly decoded in the CLI search output preview.




## [0.1.0] - 2026-03-15

### Added
- **Core Engine:** Blazing fast Markdown parsing and indexing using `tantivy`, with built-in chunking by Markdown Heading hierarchy (`#`, `##`, etc.).
- **Local-First CLI:** Full-featured command-line interface for local Vault management.
  - `larch init`: Initialize a new `.larch` vault structure safely.
  - `larch import`: Ingest single files or recursively ingest directory trees.
  - `larch search`: Term-based BM25 exact keyword search directly in your terminal.
  - `larch document`: Read specific line ranges from indexed Markdown files.
  - `larch reindex`: Force-rebuild the full index strictly to resolve path inconsistencies.
- **REST API Server:** Built-in `axum` server to expose indexing, search, and document retrieval to other HTTP clients or frontends (served on port 3000 by default).
- **Background Watcher:** High-performance, debounced filesystem listener (`notify_debouncer_full`) automatically re-indexes Markdown files seamlessly upon Save/Delete events.
- **MCP Server Protocol:** Native implementation of the Model Context Protocol (v1.2.0 over `stdio`), exposing `search` and `document` tools for Claude Desktop, Cursor, and Agentic IDEs.
- **Release Assets:** Minimum viable `Dockerfile` for self-hosted containerized RAG scenarios.

### Security
- **Path Traversal Protection (`src/document.rs`)**: Firmly rejects absolute paths out-of-vault and canonicalizes input paths against the Vault Root, preventing LLMs or HTTP requests from reading sensitive system data (e.g., `/etc/passwd`).

### Changed
- Refactored tool parameter schemas to ensure unified naming across CLI arguments, REST URL queries, and MCP Input Schemas (`query`, `limit`, `path`, `start_line`, `end_line`).
