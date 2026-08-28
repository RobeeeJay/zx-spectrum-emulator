//! The JSON the MCP server speaks, since there is no crate to speak it for us.

use zx_rustrum::mcp::json::{parse, Json};

/// What goes out comes back the same.
#[test]
fn a_value_survives_being_written_and_read_again() {
    let value = Json::obj([
        ("jsonrpc", Json::str("2.0")),
        ("id", Json::num(7)),
        (
            "result",
            Json::obj([
                ("text", Json::str("PC=$8000 \"quoted\" and a\nnewline\ttab")),
                ("empty", Json::str("")),
                (
                    "list",
                    Json::arr(vec![Json::num(0), Json::num(-1), Json::num(65535)]),
                ),
                ("yes", Json::Bool(true)),
                ("no", Json::Bool(false)),
                ("nothing", Json::Null),
            ]),
        ),
    ]);
    let written = value.to_string();
    let read = parse(&written).expect("what we wrote should parse");
    assert_eq!(read, value, "written as {written}");
}

/// Whole numbers are written as whole numbers. A T-state count that came out
/// as 69888.0 would be JSON, but nobody wants to read it and some clients
/// treat it as a different type.
#[test]
fn whole_numbers_keep_their_shape() {
    assert_eq!(Json::num(69888).to_string(), "69888");
    assert_eq!(Json::num(-32601).to_string(), "-32601");
    assert_eq!(Json::num(0.5).to_string(), "0.5");
}

/// Control characters have to be escaped or the message is not JSON. A
/// disassembly listing is full of newlines, which is how this would show up.
#[test]
fn control_characters_are_escaped() {
    let text = Json::str("line\nnext\u{1}end").to_string();
    assert_eq!(text, "\"line\\nnext\\u0001end\"");
    assert_eq!(
        parse(&text).unwrap().as_str().unwrap(),
        "line\nnext\u{1}end"
    );
}

/// Escapes on the way in, including the pair that spells a character outside
/// the basic plane.
#[test]
fn escapes_are_read_back() {
    let value = parse(r#""a\"b\\c\/d\be\ff\ng\rh\ti\u00a3j\ud83d\ude00""#).unwrap();
    assert_eq!(
        value.as_str().unwrap(),
        "a\"b\\c/d\u{8}e\u{c}f\ng\rh\ti£j😀"
    );
}

/// Numbers arrive in whatever shape the client sends them.
#[test]
fn numbers_are_read_in_every_shape_json_allows() {
    for (text, want) in [
        ("0", 0.0),
        ("-1", -1.0),
        ("32768", 32768.0),
        ("1.5", 1.5),
        ("1e3", 1000.0),
        ("-2.5E-2", -0.025),
    ] {
        assert_eq!(parse(text).unwrap().as_f64().unwrap(), want, "{text}");
    }
}

/// Nesting, whitespace and empty containers, which is what a real message is
/// made of.
#[test]
fn a_message_with_whitespace_and_empty_containers_parses() {
    let value = parse(
        r#" { "method" : "tools/call" ,
              "params": { "name": "step", "arguments": { } },
              "none": [], "id": 3 } "#,
    )
    .unwrap();
    assert_eq!(value.get("method").unwrap().as_str(), Some("tools/call"));
    assert_eq!(
        value
            .get("params")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str()),
        Some("step")
    );
    assert_eq!(value.get("none").unwrap().as_array().unwrap().len(), 0);
    assert_eq!(value.get("id").unwrap().as_i64(), Some(3));
}

/// Rubbish is refused rather than half-read: a truncated message that parsed
/// into something plausible would be answered as though it meant it.
#[test]
fn malformed_text_is_an_error_rather_than_a_guess() {
    for bad in [
        "",
        "{",
        "{\"a\"}",
        "{\"a\":}",
        "[1,]",
        "\"unterminated",
        "tru",
        "{} {}",
        "{\"a\": 1} trailing",
        "\"\\q\"",
        "\"\\u00\"",
    ] {
        assert!(parse(bad).is_err(), "{bad:?} should not parse");
    }
}
