use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
pub struct TreeNode {
    pub name: String,
    pub is_dir: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<TreeNode>>,
}

pub fn build_tree(vault_dir: &Path) -> TreeNode {
    let root_name = vault_dir.file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "/".to_string());

    let mut root = TreeNode {
        name: root_name,
        is_dir: true,
        children: Some(Vec::new()),
    };

    fn build_recursive(dir: &Path) -> Vec<TreeNode> {
        let mut children = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
            // Sort directories first, then alphabetically
            entries.sort_by_key(|e| {
                let is_dir = e.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
                (!is_dir, e.file_name())
            });

            for entry in entries {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();

                if name.starts_with('.') {
                    continue; // Skip hidden dirs like .larch, .git
                }

                if path.is_dir() {
                    let sub_children = build_recursive(&path);
                    if !sub_children.is_empty() {
                        children.push(TreeNode {
                            name,
                            is_dir: true,
                            children: Some(sub_children),
                        });
                    }
                } else if crate::utils::is_markdown(&path) {
                    children.push(TreeNode {
                        name,
                        is_dir: false,
                        children: None,
                    });
                }
            }
        }
        children
    }

    root.children = Some(build_recursive(vault_dir));
    root
}

pub fn print_tree(node: &TreeNode, prefix: &str, is_last: bool, is_root: bool) {
    if is_root {
        println!("{}", node.name);
    } else {
        let connector = if is_last { "└── " } else { "├── " };
        println!("{}{}{}", prefix, connector, node.name);
    }

    if let Some(children) = &node.children {
        let new_prefix = if is_root {
            prefix.to_string()
        } else if is_last {
            format!("{}    ", prefix)
        } else {
            format!("{}│   ", prefix)
        };

        for (i, child) in children.iter().enumerate() {
            print_tree(child, &new_prefix, i == children.len() - 1, false);
        }
    }
}