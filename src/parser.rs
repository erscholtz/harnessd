//! Tree-sitter integration for parsing and analyzing code.
//!
//! Provides cursor-to-node resolution and TODO/FIXME anchor detection across
//! the languages currently supported by the daemon.

use std::collections::HashMap;
use std::path::Path;

use tree_sitter::{Language, Node, Parser, Tree};

/// A parsed file with its AST.
pub struct ParsedFile {
    pub tree: Tree,
    pub source: String,
    pub language: SupportedLanguage,
}

/// A code region identified as an anchor point (TODO, FIXME, todo!(), etc.)
#[derive(Debug, Clone)]
pub struct Anchor {
    /// Byte range in the source file.
    pub byte_range: std::ops::Range<usize>,
    /// Type of anchor.
    pub kind: AnchorKind,
    /// Contextual information (e.g., function name, comment text).
    pub context: String,
}

/// Types of anchors we can detect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorKind {
    TodoComment,
    FixmeComment,
    TodoMacro,
    UnimplementedMacro,
    EmptyFunctionBody,
}

/// Languages currently backed by tree-sitter parsers in this daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SupportedLanguage {
    Rust,
    JavaScript,
    TypeScript,
    Tsx,
    Python,
    Go,
}

/// Multi-language parser registry keyed by file extension.
pub struct LanguageParsers {
    parsers: HashMap<SupportedLanguage, Parser>,
}

impl LanguageParsers {
    /// Create parser instances for every supported language.
    pub fn new() -> anyhow::Result<Self> {
        let mut parsers = HashMap::new();
        for &language in SupportedLanguage::all() {
            let mut parser = Parser::new();
            parser
                .set_language(&language.tree_sitter_language())
                .map_err(|e| anyhow::anyhow!("failed to set {language:?} language: {e:?}"))?;
            parsers.insert(language, parser);
        }
        Ok(Self { parsers })
    }

    /// Parse source code for the language inferred from the file path.
    pub fn parse_file(&mut self, path: &Path, source: &str) -> anyhow::Result<ParsedFile> {
        let language = SupportedLanguage::from_path(path).ok_or_else(|| {
            anyhow::anyhow!("no tree-sitter parser configured for {}", path.display())
        })?;

        let parser = self
            .parsers
            .get_mut(&language)
            .ok_or_else(|| anyhow::anyhow!("missing parser for {language:?}"))?;
        let tree = parser
            .parse(source, None)
            .ok_or_else(|| anyhow::anyhow!("parsing failed"))?;

        Ok(ParsedFile {
            tree,
            source: source.to_string(),
            language,
        })
    }

    /// Whether a file path is supported by one of the configured parsers.
    pub fn supports_path(path: &Path) -> bool {
        SupportedLanguage::from_path(path).is_some()
    }
}

impl SupportedLanguage {
    /// All languages currently supported by the parser registry.
    pub const fn all() -> &'static [SupportedLanguage] {
        &[
            SupportedLanguage::Rust,
            SupportedLanguage::JavaScript,
            SupportedLanguage::TypeScript,
            SupportedLanguage::Tsx,
            SupportedLanguage::Python,
            SupportedLanguage::Go,
        ]
    }

    /// Infer the language from a file extension.
    pub fn from_path(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "rs" => Some(Self::Rust),
            "js" | "mjs" | "cjs" | "jsx" => Some(Self::JavaScript),
            "ts" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            "py" => Some(Self::Python),
            "go" => Some(Self::Go),
            _ => None,
        }
    }

    fn tree_sitter_language(self) -> Language {
        match self {
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
            Self::Go => tree_sitter_go::LANGUAGE.into(),
        }
    }

    fn function_kinds(self) -> &'static [&'static str] {
        match self {
            Self::Rust => &["function_item", "impl_item", "method_definition"],
            Self::JavaScript => &[
                "function_declaration",
                "function",
                "method_definition",
                "arrow_function",
            ],
            Self::TypeScript | Self::Tsx => &[
                "function_declaration",
                "function",
                "method_definition",
                "arrow_function",
            ],
            Self::Python => &["function_definition"],
            Self::Go => &["function_declaration", "method_declaration"],
        }
    }

    fn supports_braced_empty_body(self) -> bool {
        !matches!(self, Self::Python)
    }

    fn line_comment_prefix(self) -> &'static str {
        match self {
            Self::Python => "#",
            _ => "//",
        }
    }
}

impl ParsedFile {
    /// Find the smallest node that contains the given byte offset.
    pub fn node_at_offset(&self, offset: usize) -> Option<Node<'_>> {
        let root = self.tree.root_node();
        find_node_at_offset(root, offset)
    }

    /// Find the enclosing function/method for a given offset.
    pub fn enclosing_function(&self, offset: usize) -> Option<Node<'_>> {
        let mut node = self.node_at_offset(offset)?;
        while let Some(parent) = node.parent() {
            if self.language.function_kinds().contains(&parent.kind()) {
                return Some(parent);
            }
            node = parent;
        }
        None
    }

    /// Find all TODO/FIXME anchors in the file.
    pub fn find_anchors(&self) -> Vec<Anchor> {
        let mut anchors = Vec::new();
        let root = self.tree.root_node();
        self.collect_anchors(root, &mut anchors);
        anchors
    }

    /// Comment prefix for this file's language.
    pub fn comment_prefix(&self) -> &'static str {
        self.language.line_comment_prefix()
    }

    fn collect_anchors(&self, node: Node<'_>, anchors: &mut Vec<Anchor>) {
        let node_kind = node.kind();
        if is_comment_node(node_kind) {
            let text = node.utf8_text(self.source.as_bytes()).unwrap_or("");
            let text_lower = text.to_lowercase();

            if text_lower.contains("todo") {
                anchors.push(Anchor {
                    byte_range: node.byte_range(),
                    kind: AnchorKind::TodoComment,
                    context: text.to_string(),
                });
            } else if text_lower.contains("fixme") {
                anchors.push(Anchor {
                    byte_range: node.byte_range(),
                    kind: AnchorKind::FixmeComment,
                    context: text.to_string(),
                });
            }
        } else if self.language == SupportedLanguage::Rust && node_kind == "macro_invocation" {
            let text = node.utf8_text(self.source.as_bytes()).unwrap_or("");
            if text.starts_with("todo!") {
                anchors.push(Anchor {
                    byte_range: node.byte_range(),
                    kind: AnchorKind::TodoMacro,
                    context: text.to_string(),
                });
            } else if text.starts_with("unimplemented!") {
                anchors.push(Anchor {
                    byte_range: node.byte_range(),
                    kind: AnchorKind::UnimplementedMacro,
                    context: text.to_string(),
                });
            }
        } else if self.language.function_kinds().contains(&node_kind)
            && self.language.supports_braced_empty_body()
        {
            if let Some(body) = node.child_by_field_name("body") {
                let body_text = body.utf8_text(self.source.as_bytes()).unwrap_or("");
                if normalized_body(body_text) == "{}" {
                    let name = node
                        .child_by_field_name("name")
                        .and_then(|name_node| name_node.utf8_text(self.source.as_bytes()).ok())
                        .unwrap_or("unknown");
                    anchors.push(Anchor {
                        byte_range: node.byte_range(),
                        kind: AnchorKind::EmptyFunctionBody,
                        context: format!("fn {}", name),
                    });
                }
            }
        }

        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                self.collect_anchors(child, anchors);
            }
        }
    }

    /// Get the source text for a node.
    pub fn node_text(&self, node: Node<'_>) -> &str {
        node.utf8_text(self.source.as_bytes()).unwrap_or("")
    }
}

fn is_comment_node(kind: &str) -> bool {
    kind.contains("comment")
}

fn normalized_body(body_text: &str) -> String {
    body_text
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
}

/// Recursively find the smallest node containing the offset.
fn find_node_at_offset(node: Node<'_>, offset: usize) -> Option<Node<'_>> {
    if !node.byte_range().contains(&offset) {
        return None;
    }

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if let Some(found) = find_node_at_offset(child, offset) {
                return Some(found);
            }
        }
    }

    Some(node)
}

/// Compute a content hash for a specific node region.
pub fn hash_node_region(source: &str, node: Node<'_>) -> String {
    let text = node.utf8_text(source.as_bytes()).unwrap_or("");
    crate::cache::compute_hash(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rust() {
        let mut parser = LanguageParsers::new().unwrap();
        let source = r#"
fn main() {
    println!("Hello, world!");
}
"#;
        let parsed = parser.parse_file(Path::new("main.rs"), source).unwrap();
        assert_eq!(parsed.tree.root_node().kind(), "source_file");
    }

    #[test]
    fn test_find_todo_comment() {
        let mut parser = LanguageParsers::new().unwrap();
        let source = r#"// TODO: implement this
fn main() {
    println!("hello");
}
"#;
        let parsed = parser.parse_file(Path::new("main.rs"), source).unwrap();
        let anchors = parsed.find_anchors();
        // Should find the TODO comment but not the function (it has a body)
        let todo_anchors: Vec<_> = anchors
            .iter()
            .filter(|a| a.kind == AnchorKind::TodoComment)
            .collect();
        assert_eq!(todo_anchors.len(), 1);
        assert_eq!(todo_anchors[0].kind, AnchorKind::TodoComment);
    }

    #[test]
    fn test_find_todo_macro() {
        let mut parser = LanguageParsers::new().unwrap();
        let source = r#"
fn main() {
    todo!("implement later");
}
"#;
        let parsed = parser.parse_file(Path::new("main.rs"), source).unwrap();
        let anchors = parsed.find_anchors();
        assert_eq!(anchors.len(), 1);
        assert_eq!(anchors[0].kind, AnchorKind::TodoMacro);
    }

    #[test]
    fn test_node_at_offset() {
        let mut parser = LanguageParsers::new().unwrap();
        let source = r#"
fn main() {
    println!("Hello");
}
"#;
        let parsed = parser.parse_file(Path::new("main.rs"), source).unwrap();

        // Find node at offset inside "println!"
        let offset = source.find("println").unwrap();
        let node = parsed.node_at_offset(offset).unwrap();
        assert!(node.kind().contains("identifier") || node.kind() == "macro_invocation");
    }

    #[test]
    fn test_find_todo_comment_in_python() {
        let mut parser = LanguageParsers::new().unwrap();
        let source = r#"def main():
    # TODO: implement this
    value = 42
"#;
        let parsed = parser.parse_file(Path::new("main.py"), source).unwrap();
        let anchors = parsed.find_anchors();
        assert_eq!(anchors.len(), 1);
        assert_eq!(anchors[0].kind, AnchorKind::TodoComment);
        assert_eq!(parsed.comment_prefix(), "#");
    }
}
