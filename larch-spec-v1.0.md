### **纯本地 Markdown AI 知识引擎 (Larch) \- 技术设计方案 v1.0**

#### **1\. 核心产品理念 (Core Philosophy)**

* **绝对数据主权 (Local-First)**：以本地文件系统为唯一真理源（Single Source of Truth），所有知识以纯明文 .md 格式集中存储，天生支持 Git 版本控制与人类无障碍阅读。  
* **物理完整，逻辑切分 (Logical Chunking)**：针对 AI 检索需求，绝不破坏原文件物理结构。在内存中通过解析 AST 进行逻辑分块，并通过“行号映射”在索引库中锚定原文片段。  
* **微内核与插件化解耦 (Microkernel Architecture)**：核心 CLI 守护进程仅负责“目录监听 \+ 构建索引 \+ 暴露接口”。外部数据拉取（如飞书同步）与前端展示（如 Raycast/AI 客户端）全部作为松耦合插件存在。

#### **2\. 系统架构分层 (Architecture Layers)**

* **存储层 (Storage Layer)**  
  * **集中式 Vault (工作区)**：强制设定统一的根目录（如 \~/.larch\_vault），所有知识资产必须 Copy/Move 至此，杜绝“原地索引”带来的游离指针与监听风暴问题。全局单一实例，所有操作只针对此目录内，目录内支持多级子目录，在导入文件时可指定，当然也可以手动操作目录，这样需要通过后面的监听模块保证索引的正确性；  
* **核心引擎层 (Core Engine \- Rust Daemon)**  
  * **文件监听模块**：实时监控 Vault 目录树，集成防抖（Debounce）与批处理机制。  
  * **解析切块模块**：读取 .md，根据标题层级（Heading）精确提取逻辑 Chunk 及行号元数据。  
  * **检索引擎模块**：维护全局单例写入锁（Single Writer），将 Chunk 落盘为倒排索引。  
* **接口与交互层 (Interface Layer)**  
  * **CLI 命令行**：提供人工直接调用的终端命令。  
  * **API 服务**：提供 RESTful 与 MCP (Model Context Protocol) 协议接口。

#### **3\. 核心技术栈选型 (Tech Stack)**

* **开发语言**：**Rust**。编译为单文件二进制可执行程序，极度轻量，开箱即用。  
* **检索引擎**：**tantivy**。Rust 生态顶级的全文检索引擎，毫秒级响应，支持自定义 Schema 与 MVCC 并发读取。  
* **Markdown 解析**：**pulldown-cmark**。基于事件流（Pull Parser）的解析器，零拷贝（Zero-copy）提取文本块，自带精确的字符/行号偏移量。  
* **Web 与路由框架**：**axum**。提供高性能异步 HTTP 服务。  
* **目录监听**：**notify**。跨平台文件系统事件监控引擎。

#### **4\. 核心数据结构 (Tantivy Schema)**

这是连接“物理明文”与“AI 检索”的桥梁：

* file\_path (String, Indexed \+ Stored)：物理文件绝对路径（用于整文件召回）。  
* chunk\_id (String, Indexed \+ Stored)：块唯一标识（如 文件hash\_起始行）。  
* title\_hierarchy (String, Indexed)：当前块所属的层级标题路径（如 架构 \> 数据库选型 \> SQLite）。  
* content (Text, Indexed)：经过分词的正文切块（搜索命中的核心字段）。  
* start\_line / end\_line (u64, Stored)：在源文件中的物理行号（精准定位与切片提取）。  
* keywords (String array, Indexed)：提取的高频词或自定义 Meta Tags。

#### **5\. 接口与路由设计 (API & CLI)**

**A. CLI 命令行交互**

* larch init \<dir\>：在给定目录初始化 Vault，如非空目录，则深层完成初始化索引。  
* larch serve：启动后台守护进程（拉起监听、REST API 与 MCP Server）。  
* larch search "\<关键词\>"：终端极速检索，高亮输出命中片段及行号。  
* larch import \<文件/URL\>：将外部文件 Copy 入 Vault，并立即触发切块与索引。  
  * \-x：剪切移动  
  * \-d –dir：file dir，形式 path/to/dir，支持多级，不存在会自动创建；

**B. RESTful API (面向普通前端/效率插件)**

* GET /api/v1/search?q=\<keyword\>\&limit=10：执行关键词检索，返回匹配的 Chunk 列表。  
* GET /api/v1/document?path=\<file\_path\>\&start\_line=x\&end\_line=y：按行号精准读取文件片段的明文。  
* POST /api/v1/import：接收外部插件（如Web UI、飞书同步脚本）推送的 Markdown 内容，落盘并建库。

**C. MCP 协议接口 (面向 AI 客户端如 Claude/OpenClaw)**

* Tool: search\_local\_knowledge：允许 AI 主动传入关键词，获取相关知识片段。  
* Tool: read\_file\_context：允许 AI 根据搜索结果中的 file\_path 和行号，回源读取更完整的上下文。

6\. 实现要求

- 追求代码的简洁和轻量化，避免复杂繁重的实现  
- 以上技术设计仅供基本参考，API 设计和实现充分考虑实际使用场景，可以进一步扩展  
- 有任何问题随时继续讨论，或在spec内留下疑问，我来逐条回答

