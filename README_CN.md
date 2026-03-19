# Larch 🌲

**[English](README.md) | [中文](README_CN.md)**

**本地优先的 Markdown AI 知识引擎。**

Larch 是一个高性能、轻量级、本地优先的知识库引擎，专为将你的 Markdown 笔记通过 **MCP（Model Context Protocol）** 和标准 REST API 提供给 AI Agent 和大语言模型（LLM）而设计。

基于 Rust 构建，使用 **Tantivy** 实现高性能全文检索，支持中日韩（CJK）分词。它在后台静默运行，将你的本地文件夹变成一个可被 AI 即时查询的知识大脑。

---

## 🚀 特性

- **本地优先 & 安全**：所有数据在本地运行和存储。无云端同步、无向量数据库、无隐藏 API 费用。
- **极速搜索**：基于 Tantivy 的 BM25 全文检索，支持 Jieba 中文分词，精准匹配关键词并快速返回上下文。
- **原生 MCP 服务**：暴露标准 MCP 工具（`search`、`document`、`import` 等），可与 Claude Desktop、Cursor 或任何 MCP 兼容的 LLM 前端无缝集成。
- **REST API & 文件监听**：内置 HTTP 服务和文件系统监听（`inotify`/`FSEvents`）。在 Obsidian 或 VSCode 中修改 Markdown 文件，自动实时索引。
- **智能分块**：自动解析 Markdown Frontmatter（YAML）并按标题层级拆分文档，保留语义上下文。

## 📦 安装

### 从源码编译（推荐）
```bash
git clone git@code.byted.org:tiktok/larch.git
cd larch
cargo build --release
cp target/release/larch /usr/local/bin/
```

### 预编译二进制
运行构建脚本可生成 macOS (arm64) 和 Linux (x86_64/arm64) 的预编译包：
```bash
./scripts/build-release.sh
```
也可以从 [Releases 页面](https://code.byted.org/tiktok/larch/-/releases) 下载并解压到 PATH 中。

## 🛠️ 使用

Larch 默认使用 `~/.larch` 作为 Vault（知识库目录），支持通过 `larch init <path>` 自定义路径。

### 1. 初始化 Vault
```bash
larch init
# 或指定自定义路径
larch init ~/my-vault
```

### 2. 导入笔记
导入单个文件或整个目录：
```bash
larch import ~/Notes/draft.md
# 递归导入整个目录
larch import ~/Documents/MyBrain/
```

### 3. 搜索与检索
```bash
larch search "Rust 所有权"
# 按标签或目录过滤
larch search "Rust" --tag "programming" --dir "tech/notes"

# 读取文件指定行范围
larch document inbox/draft.md --start-line 10 --end-line 20
```

### 4. 探索 Vault
```bash
# 查看目录树
larch tree

# 列出所有标签
larch tag ls

# 查看某个标签下的文件
larch tag ls "architecture"
```

### 5. 启动引擎（REST API & 文件监听）
启动文件监听（自动索引变更）和 HTTP API：
```bash
larch serve --port 3000
```

同时启用 MCP stdio 服务（推荐用于 AI Agent 集成）：
```bash
larch serve --port 3000 --mcp
```

*提示：可通过 `nohup larch serve > ~/.larch/logs/larch.log 2>&1 &` 后台运行，或使用 `systemd`/`pm2` 管理。*

### 6. 重建索引
索引过期或升级 Larch 后，重建索引：
```bash
larch reindex
```
*注：若 `larch serve` 正在运行，reindex 会自动通过 HTTP 委托给服务端执行。*

## 🛠️ 开发

```bash
git clone git@code.byted.org:tiktok/larch.git
cd larch
cargo build --release
```

交叉编译多平台：
```bash
./scripts/build-release.sh
```

---

## 🤖 MCP 集成

Larch 是原生 MCP 服务端，允许 AI Agent（如 Claude、Cursor）自主探索你的本地知识库，基于 MCP SDK 的 `stdio` 传输。

**推荐使用 `larch serve --mcp`**，在单进程中同时运行 REST API 和 MCP 服务，共享写入锁：

编辑 `claude_desktop_config.json`：
```json
{
  "mcpServers": {
    "larch": {
      "command": "larch",
      "args": ["serve", "--mcp"]
    }
  }
}
```

`larch mcp` 仍可作为独立 MCP 服务运行，但与 `larch serve` 同时运行可能导致写入冲突。

### 可用 MCP 工具：
- `search`：关键词搜索知识库，支持 `tag` 和 `dir` 过滤。
- `document`：读取 Markdown 文件的指定行范围。
- `tree`：获取 Vault 完整目录结构。
- `tags`：列出所有标签或查找特定标签下的文档。
- `import`：导入 Markdown 内容到 Vault，自动处理资源文件并索引。

---

## 🌐 REST API 参考

运行 `larch serve` 后，以下端点可用（默认端口 `3000`）：

| 端点 | 方法 | 说明 |
| :--- | :--- | :--- |
| `/api/v1/search` | `GET` | 全文搜索，支持 `query`、`limit`、`tag`、`dir` 参数。 |
| `/api/v1/tree` | `GET` | 返回 Vault 完整目录结构（JSON）。 |
| `/api/v1/tags` | `GET` | 列出标签。使用 `?tag=name` 获取特定标签下的文件。 |
| `/api/v1/document` | `GET` | 通过 `path`、`start_line`、`end_line` 获取文件内容。 |
| `/api/v1/import` | `POST` | 通过 JSON body `{ filename, content, dir? }` 导入内容。 |
| `/api/v1/import/file` | `POST` | 从磁盘导入，body `{ source_path, move_file?, dir? }`。 |
| `/api/v1/reindex` | `POST` | 重建整个搜索索引。 |
| `/health` | `GET` | 检查服务状态和 Vault 路径。 |

---

## 🐳 Docker 部署

Larch 主要作为本地 CLI 工具优化，但也提供 `Dockerfile` 用于自托管 RAG 架构或隔离环境部署（如搭配 Dify、NextChat）。

```bash
docker build -t larch:latest .
docker run -d \
  -v ~/.larch:/root/.larch \
  -p 3000:3000 \
  --name larch-server \
  larch:latest
```
*注：由于 Docker 的网络和卷隔离，挂载卷上的文件系统监听事件在部分宿主机操作系统（如 Windows/macOS）上可能不够可靠。*

---

## 🗺️ 路线图

- [x] **自定义 Vault 路径**：支持 `larch init <path>` 初始化到自定义目录。
- [ ] **智能分块增强**：改进解析器对代码块和嵌套列表的处理。
- [ ] **生态：预处理提取器**：提供脚本从飞书文档、Notion 等同步并转换为 Larch 兼容的本地 Markdown。
- [ ] **生态：AI UI 集成**：支持与 NextChat、LobeChat、Dify 等主流 LLM Chat UI 的无缝对接，将 Larch 作为 RAG 后端。

---

## 📄 许可证

本项目基于 MIT 许可证开源 — 详见 [LICENSE](LICENSE) 文件。
