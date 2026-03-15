### **Larch 元数据与智能摘要设计规范 (Metadata & AI Summarization Spec)**

本规范定义了 Larch 知识引擎如何处理 Markdown 文件的非标准元数据（如标签、摘要、版本号），以及如何优雅地集成 AI 总结能力。

#### **1\. 元数据存储标准：YAML Frontmatter**

Larch 采用业界事实标准 **YAML Frontmatter** 来承载结构化元数据。所有由插件生成的 Markdown 文件，头部需包含由 `---` 包裹的 YAML 块。

**标准数据结构示例：**

YAML  
\---  
title: "Rust 并发模型解析"  
date: 2026-03-14T20:58:00+08:00  
tags: \[rust, 并发, 架构设计\]  
summary: "本文探讨了 RwLock 在读多写少场景下的优势，并附带了死锁排查指南。"  
version: "1.0.0"  
\---  
正文内容...

#### **2\. 架构解耦原则：AI 能力外置**

Larch 的核心 CLI 进程**绝不直接调用**任何大模型 API。所有的 AI 摘要生成、关键词提炼等重度/不稳定操作，必须在\*\*数据输入层（外部插件）\*\*完成。

**标准工作流：**

1. **外部插件工作（如飞书同步脚本）：**  
   * 从飞书 API 拉取原始文档文本。  
   * 脚本调用外部 LLM API，传入正文，要求输出摘要（Summary）和提取的标签（Tags）。  
   * 脚本将生成的 YAML 元数据与纯文本组装成标准 Markdown 格式。  
2. **Larch 核心工作：**  
   * 脚本调用 `larch import` 或将文件写入 Vault 目录。  
   * Larch 仅执行纯本地的解析、物理落盘和建立索引，耗时保持在毫秒级。

#### **3\. 标签 (Tags) 统一处理策略**

Larch 的检索引擎将合并两种来源的标签，统一写入 Tantivy 的 `keywords` 字段，提升基于标签的聚合搜索召回率。

* **显式标签（规范源）：** 直接解析 YAML 头部的 `tags` 数组。  
* **隐式标签（灵活源）：** 在使用 `pulldown-cmark` 解析正文 Chunk 时，通过正则表达式（如 `r"#([^\s#]+)"`）提取行内标签（例如正文里随手写的 `#性能优化`）。

#### **4\. Larch 核心引擎适配指南**

为了支持上述规范，Larch 底层引擎需做如下微调：

* **依赖库扩展：** 引入 `gray_matter` 或 `matter`（Rust 生态中专门用于剥离和解析 Markdown Frontmatter 的成熟库）。在交由 `pulldown-cmark` 处理正文前，先剥离 YAML。  
* **Tantivy Schema 扩充：**  
  * 新增 `summary` 字段（类型：Text，配置为 Stored 但可选择不分词 Indexed，仅用于搜索结果列表的快速预览）。  
  * 将解析出的 Explicit Tags 和 Implicit Tags 去重合并后，存入现有的 `keywords` 字段。  
  * 新增 `version` 和 `created_at` 字段（类型：String/I64，用于排序和元数据过滤）。
