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
