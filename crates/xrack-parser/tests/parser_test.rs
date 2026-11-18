#![cfg(test)]

use xrack_parser::{ast::Diagnostic, parse};

const SYNTAX: &str = include_str!("fixtures/syntax.xr");
const EXPECTED_AST: &str = include_str!("fixtures/syntax.ast.json");

fn check_ast(source: &str) {
    let expected: serde_json::Value = serde_json::from_str(EXPECTED_AST).unwrap();
    assert_eq!(
        serde_json::to_value(parse(source).unwrap()).unwrap(),
        expected
    );
}

#[test]
fn parses_the_reference_ast() {
    check_ast(SYNTAX);
}

#[test]
fn bom_and_crlf_preserve_ast_positions() {
    check_ast(&format!("\u{feff}{}", SYNTAX.replace('\n', "\r\n")));
}

#[test]
fn reports_reference_lexer_and_parser_diagnostics() {
    let cases: Vec<(String, Vec<Diagnostic>)> =
        serde_json::from_str(include_str!("fixtures/diagnostics.json")).unwrap();
    for (source, expected) in cases {
        assert_eq!(parse(&source).unwrap_err(), expected, "{source:?}");
    }
}

#[test]
fn record_update_base_must_follow_the_explicit_fields() {
    for (value, valid) in [
        ("User { name: \"new\", ..root.base }", true),
        ("User { ..root.base, }", true),
        ("User { name: \"new\" ..root.base }", false),
        ("User { ..root.base, name: \"new\" }", false),
        ("User { ..root.base, ..root.base }", false),
        ("User { .. }", false),
    ] {
        let source = format!(
            "struct User {{ name: string }} component App inherits Window {{ property <User> current: {value}; }}"
        );
        assert_eq!(parse(&source).is_ok(), valid, "{value}");
    }
}

#[test]
fn parses_nested_callback_branches_and_loops() {
    let source = r#"
        component Demo inherits Window {
            callback run(bool);
            run(ready) => {
                if ready {} else if false {
                    for item[index] in ["a", "b"] {
                        if index == 0 { Log.info(item); }
                    }
                } else {
                    for index in 3 {}
                }
            }
        }
    "#;
    assert!(parse(source).is_ok(), "{:?}", parse(source).err());
    assert!(parse(&source.replace("else if false", "else if")).is_err());
}
