# Larch 🌲

**Local-first Markdown AI Knowledge Engine.**

Larch is a blazingly fast, lightweight, and local-first knowledge base engine designed specifically to serve your Markdown notes to AI Agents and Large Language Models (LLMs) via the **Model Context Protocol (MCP)** and a standard REST API.

Built in Rust, it uses **Tantivy** for high-performance full-text search and is designed to run silently in the background of your OS, turning your local folder into an instantly queryable AI brain.

---

## 🚀 Features

- **Local-First & Secure**: Everything runs locally. No cloud sync, no vector databases, no hidden API costs.
- **Lightning Fast Search**: Powered by Tantivy, offering BM25 token-based full-text retrieval tailored for exact keyword matching and fast context retrieval.
- **MCP Server Native**: Exposes standard MCP tools (`search`, `document`) allowing seamless integration with Claude Desktop, Cursor, or any MCP-compatible LLM frontend.
- **REST API & Watcher**: A built-in HTTP server with file-system watching (`inotify`/`FSEvents`). Modify your markdown files in Obsidian or VSCode, and they are indexed instantly.
- **Smart Chunking**: Automatically parses Markdown Frontmatter (YAML) and splits documents by heading hierarchies, preserving semantic context.

## 📦 Installation

Larch provides pre-compiled standalone binaries. You do not need Rust installed.

### Quick Install (macOS / Linux)
The easiest way to install Larch is via our official installation script:

```bash
curl -LsSf https://raw.githubusercontent.com/Joker-Jelly/larch/main/install.sh | sh
```

### Manual Download
1. Go to the [Releases page](https://github.com/Joker-Jelly/larch/releases/latest) on GitHub.
2. Download the binary archive for your operating system and architecture.
3. Extract the `larch` executable and move it to your path (e.g. `/usr/local/bin`).

## 🛠️ Usage

Larch uses `~/.larch` as your default Vault (knowledge base folder). 

### 1. Initialize your Vault
```bash
larch init
```
This creates the `.larch` folder and required internal directories (`assets/`, `index/`).

### 2. Import your Existing Notes
Import a single file or an entire directory of notes into the vault.
```bash
larch import ~/Notes/draft.md
# Or import a whole folder recursively
larch import ~/Documents/MyBrain/
```

### 3. Search and Retrieve
Quickly verify that your notes are indexed using the CLI:
```bash
larch search "Rust ownership"
larch document inbox/draft.md --start-line 10 --end-line 20
```

### 4. Start the Engine (REST API & File Watcher)
This will start the file watcher (to auto-index new changes) and expose the HTTP API on port `3000`.
```bash
larch serve --port 3000
```
*Tip: Run this in the background using `nohup larch serve > ~/.larch/logs/larch.log 2>&1 &` or manage it via `systemd`/`pm2`.*

## 🛠️ Development (Build from Source)

If you prefer to compile Larch yourself or want to contribute to the project, you'll need [Rust and Cargo](https://rustup.rs/) installed:

```bash
git clone https://github.com/Joker-Jelly/larch.git
cd larch
cargo build --release
```

---

## 🤖 MCP Integration (Model Context Protocol)

Larch shines when connected to an AI Agent. Larch implements the MCP SDK over `stdio`. 

**To use Larch with Claude Desktop:**

Edit your `claude_desktop_config.json`:
```json
{
  "mcpServers": {
    "larch": {
      "command": "/path/to/your/larch",
      "args": ["mcp"]
    }
  }
}
```
Now, whenever you ask Claude to "Search my local notes about Rust", it will autonomously query Larch and read specific file chunks into its context window!

---

## 🐳 Docker Deployment

While Larch is optimized as a local CLI tool, a `Dockerfile` is provided for self-hosted RAG architectures or deploying within isolated environments (e.g., alongside Dify or NextChat).

```bash
docker build -t larch:latest .
docker run -d \
  -v ~/.larch:/root/.larch \
  -p 3000:3000 \
  --name larch-server \
  larch:latest
```
*Note: Due to Docker's networking and volume isolation, OS-level file watcher events on mounted volumes may not trigger reliably on all host operating systems (e.g., Windows/macOS).*

---

## 🗺️ Roadmap / TODOs

- [ ] **Custom Vault Paths:** Allow `larch init <path>` to initialize the `.larch` database in a user-defined directory (like an existing Obsidian or Logseq vault) rather than hardcoding `~/.larch`.
- [ ] **Smart Chunking Improvements:** Enhance the parser to better understand code blocks and nested bullet points.
- [ ] **Larch Ecosystem: Pre-processing Extractors:** Provide official peripheral scripts (e.g., Python/Node CLI) to sync and convert documents from Lark Doc and Notion into Larch-compatible local Markdown.
- [ ] **Larch Ecosystem: Post-processing AI UI:** Curate and officially support seamless integrations with popular LLM Chat UIs (like NextChat, LobeChat, Dify) utilizing Larch's REST API and MCP capabilities as their primary RAG backend.

---

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
