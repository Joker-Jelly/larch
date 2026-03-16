# Larch v0.2.0 实施计划

## 1. 索引结构与权重优化

### 1.1 `FileMeta` 与 `Chunk` 数据结构调整
*   **FileMeta (YAML frontmatter)**：新增 `keywords: Vec<String>` 字段。
*   **Chunk**：
    *   移除原有 `keywords` 字段，新增 `tags: Vec<String>`（合并 YAML 的 `tags` 与正文的内联 `#tags`）。
    *   新增 `keywords: Vec<String>` 字段，专门用于透传 `FileMeta` 中的 `keywords` 数据。

### 1.2 `src/index.rs` Schema 及索引写入修改
*   **新增字段**：
    *   `dirs`: 采用 `Facet` 格式 (`builder.add_facet_field("dirs")`)。基于 `file_path` 生成 Facet 路径（例如 `a/b/c.md` -> `/a/b`）。
    *   `tags`: 采用多值文本格式并附加 FAST 属性，用于高效过滤。
*   **调整 keywords 字段**：
    *   其 Indexing 配置保留，在 `index_file` 时数据来源仅为 `FileMeta.keywords`。
*   **搜索权重优化**：
    *   修改 `QueryParser` 的初始化逻辑，使其权重满足：`keywords > title_hierarchy > content`（分别设置 boost 权重，如 `3.0`, `2.0`, `1.0`）。

---

## 2. API / MCP / CLI Tool 扩充

### 2.1 【search】 过滤条件扩充
*   **CLI**: 新增 `--tag <TAG>` 和 `--dir <DIR>` 参数。
*   **API**: `SearchQuery` 新增 `tag: Option<String>` 和 `dir: Option<String>`。
*   **MCP**: `SearchArgs` 新增 `tag: Option<String>` 和 `dir: Option<String>`。
*   **检索逻辑**：在 `search` 函数中，将 `tag` 和 `dir` 作为可选参数，组合成 `BooleanQuery` 并取交集 (`Occur::Must`)。

### 2.2 【tree】 目录树功能
*   **设计原则**：不走索引，直接通过文件 I/O (`WalkDir`) 遍历。
*   **数据结构**：
    ```rust
    pub struct TreeNode {
        pub name: String,
        pub is_dir: bool,
        pub children: Option<Vec<TreeNode>>,
    }
    ```
*   **核心逻辑**：在 `src/tree.rs` 中实现 `build_tree`。
*   **多端适配**：
    *   `CLI`: 新增 `tree` 命令。无 `--json` 时在终端输出格式化图形树，带 `--json` 输出 JSON。
    *   `API`: 新增 `GET /api/v1/tree`。
    *   `MCP`: 新增 `tree` tool。

### 2.3 【tag】 标签聚合功能
*   **核心逻辑**：
    *   通过遍历 Tantivy 索引，聚合所有标签及对应的文档路径。
    *   提供接口：给定可选 `tag_name`，为空返回全部标签及关联文件列表；有值时返回对应标签的文件列表。
*   **多端适配**：
    *   `CLI`: 新增 `tag` 命令（如 `tag ls <tag>`）。支持文本排版打印及 `--json` 格式。
    *   `API`: 新增 `GET /api/v1/tags`。
    *   `MCP`: 新增 `tags` tool。

---

## 4. 额外体验与工程优化 (Additional Refinements)
在实现 0.2.0 基础规划后，为了提升工程质量和终端体验，我们额外实施了以下优化：

### 4.1 CLI 搜索摘要高亮 (Snippet Generator)
* **现状改进**：废弃了原先对 `content` 粗暴截取前 200 字符的做法。
* **实现**：引入 Tantivy 的 `SnippetGenerator`，能够在命中的关键词周围智能截取上下文，并在终端通过 ASCII ANSI 码（`\x1b[1;31m`）高亮显示匹配关键字。同时加入了 HTML Entity 反转义（如 `&lt;` 转为 `<`），确保特殊字符在控制台展示自然。

### 4.2 标签聚合性能优化 (Fast Fields 提速)
* **现状改进**：查询 `tag ls` 时不再需要反序列化完整的索引文档库。
* **实现**：由于 `tags` 已经声明为带有 FAST 属性的字段，我们改为直接通过 `SegmentReader` 获取 `inverted_index` 并遍历 `TermDictionary`。这使得我们能以毫秒级的极低延迟快速聚合出数万篇文档中的所有唯一标签。

### 4.3 代码结构清理与去重 (DRY Refactoring)
* **抽取 Utils**：将散落在不同文件中（如 `main.rs`, `tree.rs`）判断是否为 Markdown 文件的冗余逻辑 `path.extension() == "md"`，统一抽取至 `src/utils.rs` 的 `is_markdown` 函数。
* **上下文初始化宏**：在 `src/main.rs` 的各 CLI 子命令中抽取了 `init_context()` 函数，统一处理 `VaultConfig` 与 `Index` 的打开逻辑，去除了大量样板代码。

### 4.4 核心 Bug 修复
* **解析器偏移量 Bug**：修复了当 Markdown 文件头部**没有** YAML Frontmatter 时，`gray_matter` 截断前导空白/符号导致我们的 `body` 字节偏移量错位，从而引发丢失文件第一个字符（比如标题的 `#`）或 panic 的问题。
* **CLI 子命令识别**：规范化了 `tag` 的子命令，引入了 `TagCommands::Ls` 以准确响应 `larch tag ls` 而不会将其误解析为一个名叫 `"ls"` 的 Tag 搜索行为。