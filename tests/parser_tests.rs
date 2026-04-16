//! Integration tests for the tree-sitter parser module.

use std::path::Path;

use harnessd::parser::{AnchorKind, LanguageParsers};

#[test]
fn test_parse_rust_source_file() {
    let mut parser = LanguageParsers::new().expect("failed to create parser");
    let source = r#"fn main() {
    println!("Hello, world!");
}
"#;

    let parsed = parser
        .parse_file(Path::new("main.rs"), source)
        .expect("failed to parse");
    let root = parsed.tree.root_node();
    assert_eq!(root.kind(), "source_file");
}

#[test]
fn test_node_at_offset() {
    let mut parser = LanguageParsers::new().expect("failed to create parser");
    let source = r#"fn main() {
    println!("Hello");
}
"#;

    let parsed = parser
        .parse_file(Path::new("main.rs"), source)
        .expect("failed to parse");

    // Find offset of "println"
    let offset = source.find("println").expect("println not found");
    let node = parsed.node_at_offset(offset).expect("no node at offset");

    // The node should be an identifier or part of the macro invocation
    let node_text = parsed.node_text(node);
    assert!(node_text.contains("println") || node.kind().contains("identifier"));
}

#[test]
fn test_enclosing_function() {
    let mut parser = LanguageParsers::new().expect("failed to create parser");
    let source = r#"fn outer() {
    fn inner() {
        let x = 42;
    }
}
"#;

    let parsed = parser
        .parse_file(Path::new("main.rs"), source)
        .expect("failed to parse");

    // Find offset of "42"
    let offset = source.find("42").expect("42 not found");
    let function = parsed
        .enclosing_function(offset)
        .expect("no enclosing function");

    let func_text = parsed.node_text(function);
    assert!(func_text.contains("inner"));
    assert!(func_text.contains("42"));
}

#[test]
fn test_find_todo_comment() {
    let mut parser = LanguageParsers::new().expect("failed to create parser");
    let source = r#"// TODO: implement this function
fn main() {
    println!("hello");
}
"#;

    let parsed = parser
        .parse_file(Path::new("main.rs"), source)
        .expect("failed to parse");
    let anchors = parsed.find_anchors();

    // Should find the TODO comment but not the function (it has a body)
    let todo_anchors: Vec<_> = anchors
        .iter()
        .filter(|a| a.kind == AnchorKind::TodoComment)
        .collect();
    assert_eq!(todo_anchors.len(), 1);
    assert_eq!(todo_anchors[0].kind, AnchorKind::TodoComment);
    assert!(todo_anchors[0].context.contains("TODO"));
}

#[test]
fn test_find_fixme_comment() {
    let mut parser = LanguageParsers::new().expect("failed to create parser");
    let source = r#"// FIXME: this is broken
fn main() {
    println!("fix me");
}
"#;

    let parsed = parser
        .parse_file(Path::new("main.rs"), source)
        .expect("failed to parse");
    let anchors = parsed.find_anchors();

    // Should find the FIXME comment but not the function (it has a body)
    let fixme_anchors: Vec<_> = anchors
        .iter()
        .filter(|a| a.kind == AnchorKind::FixmeComment)
        .collect();
    assert_eq!(fixme_anchors.len(), 1);
    assert_eq!(fixme_anchors[0].kind, AnchorKind::FixmeComment);
    assert!(fixme_anchors[0].context.contains("FIXME"));
}

#[test]
fn test_find_todo_macro() {
    let mut parser = LanguageParsers::new().expect("failed to create parser");
    let source = r#"fn main() {
    todo!("implement later");
}
"#;

    let parsed = parser
        .parse_file(Path::new("main.rs"), source)
        .expect("failed to parse");
    let anchors = parsed.find_anchors();

    assert_eq!(anchors.len(), 1);
    assert_eq!(anchors[0].kind, AnchorKind::TodoMacro);
    assert!(anchors[0].context.contains("todo!"));
}

#[test]
fn test_find_unimplemented_macro() {
    let mut parser = LanguageParsers::new().expect("failed to create parser");
    let source = r#"fn main() {
    unimplemented!("not yet");
}
"#;

    let parsed = parser
        .parse_file(Path::new("main.rs"), source)
        .expect("failed to parse");
    let anchors = parsed.find_anchors();

    assert_eq!(anchors.len(), 1);
    assert_eq!(anchors[0].kind, AnchorKind::UnimplementedMacro);
    assert!(anchors[0].context.contains("unimplemented!"));
}

#[test]
fn test_find_empty_function_body() {
    let mut parser = LanguageParsers::new().expect("failed to create parser");
    let source = r#"fn empty_function() {}
"#;

    let parsed = parser
        .parse_file(Path::new("main.rs"), source)
        .expect("failed to parse");
    let anchors = parsed.find_anchors();

    assert_eq!(anchors.len(), 1);
    assert_eq!(anchors[0].kind, AnchorKind::EmptyFunctionBody);
    assert!(anchors[0].context.contains("empty_function"));
}

#[test]
fn test_find_multiple_anchors() {
    let mut parser = LanguageParsers::new().expect("failed to create parser");
    let source = r#"// TODO: first task
fn func1() {
    todo!("implement");
}

// FIXME: second task
fn func2() {}
"#;

    let parsed = parser
        .parse_file(Path::new("main.rs"), source)
        .expect("failed to parse");
    let anchors = parsed.find_anchors();

    assert_eq!(anchors.len(), 4);

    let todo_comments: Vec<_> = anchors
        .iter()
        .filter(|a| a.kind == AnchorKind::TodoComment)
        .collect();
    assert_eq!(todo_comments.len(), 1);

    let fixme_comments: Vec<_> = anchors
        .iter()
        .filter(|a| a.kind == AnchorKind::FixmeComment)
        .collect();
    assert_eq!(fixme_comments.len(), 1);

    let todo_macros: Vec<_> = anchors
        .iter()
        .filter(|a| a.kind == AnchorKind::TodoMacro)
        .collect();
    assert_eq!(todo_macros.len(), 1);

    let empty_functions: Vec<_> = anchors
        .iter()
        .filter(|a| a.kind == AnchorKind::EmptyFunctionBody)
        .collect();
    assert_eq!(empty_functions.len(), 1);
}

#[test]
fn test_case_insensitive_todo() {
    let mut parser = LanguageParsers::new().expect("failed to create parser");
    let source = r#"// todo: lowercase
fn main() {
    println!("hi");
}
"#;

    let parsed = parser
        .parse_file(Path::new("main.rs"), source)
        .expect("failed to parse");
    let anchors = parsed.find_anchors();

    // Should find the todo comment but not the function (it has a body)
    let todo_anchors: Vec<_> = anchors
        .iter()
        .filter(|a| a.kind == AnchorKind::TodoComment)
        .collect();
    assert_eq!(todo_anchors.len(), 1);
    assert_eq!(todo_anchors[0].kind, AnchorKind::TodoComment);
}

#[test]
fn test_no_anchors_in_normal_code() {
    let mut parser = LanguageParsers::new().expect("failed to create parser");
    let source = r#"fn main() {
    let x = 42;
    println!("{}", x);
}

fn helper() -> i32 {
    123
}
"#;

    let parsed = parser
        .parse_file(Path::new("main.rs"), source)
        .expect("failed to parse");
    let anchors = parsed.find_anchors();

    // Should not detect normal functions as empty (they have bodies)
    assert!(anchors.is_empty());
}

#[test]
fn test_block_comment_todo() {
    let mut parser = LanguageParsers::new().expect("failed to create parser");
    let source = r#"/* TODO: handle this case */
fn main() {
    println!("hi");
}
"#;

    let parsed = parser
        .parse_file(Path::new("main.rs"), source)
        .expect("failed to parse");
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
fn test_node_text_extraction() {
    let mut parser = LanguageParsers::new().expect("failed to create parser");
    let source = r#"fn test_function() {
    let x = 42;
}
"#;

    let parsed = parser
        .parse_file(Path::new("main.rs"), source)
        .expect("failed to parse");
    let root = parsed.tree.root_node();

    // The root node text should be the entire source
    let root_text = parsed.node_text(root);
    assert_eq!(root_text, source);
}

#[test]
fn test_python_todo_comment() {
    let mut parser = LanguageParsers::new().expect("failed to create parser");
    let source = r#"def main():
    # TODO: implement
    value = 42
"#;

    let parsed = parser
        .parse_file(Path::new("main.py"), source)
        .expect("failed to parse");
    let anchors = parsed.find_anchors();

    assert_eq!(anchors.len(), 1);
    assert_eq!(anchors[0].kind, AnchorKind::TodoComment);
}
