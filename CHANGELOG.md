# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
