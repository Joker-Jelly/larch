# 索引结构优化、工具扩充

- 索引结构改动：在索引 schema 中增加 tags 和 dirs 两个索引字段，用于索引标签和目录结构；dir用Facet数据格式，tag由于只是扁平结构，采用多值字符串字段（Multi-valued String） + FAST 属性；原有keywords字段，改成类似summary字段的实现，只取meta data中的keywords，预留给未来AI自动提炼关键词后的检索（属于正文的一部分）
- 优化正文索引权重，命中分值 keywords > title_hierarchy > content
- CLI Tool/API/MCP 改动：
  - 【search】 增加 tag 和 dir 的两重过滤条件，默认为空，即不过滤；
  - 【tree】新一级命令，用于输出完整文件目录树结构，CLI 返回命令行图形树状表达并支持 --json 返回 JSON 结构，API 和 MCP 默认返回JSON结构；节点参考 pub struct TreeNode { name: String, is\_dir: bool, children: Option\<Vec<TreeNode>>,} 来设计；实现使用文件io来实现，不用走 facet 的过滤聚合；
  - 【tag】新一级命令，优先支持 tag ls <tag>，tag参数可选，为空时候打印所有标签 和 对应文档路径，给定tag后打印指定标签的所有文档路径，在cli以结构化图形表达，支持--json返回；API 为 api/v1/tags ，MCP 为 tags ，功能相同，直接以json返回；