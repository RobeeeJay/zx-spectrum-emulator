//! The ZX81 through the MCP server. A different machine, not a Spectrum with
//! less in it, and the tools say so where they have nothing to report.

use zx_rustrum::mcp::json::{parse, Json};
use zx_rustrum::mcp::Server;

fn call(server: &mut Server, tool: &str, args: Json) -> Result<String, String> {
    let line = Json::obj([
        ("jsonrpc", Json::str("2.0")),
        ("id", Json::num(1)),
        ("method", Json::str("tools/call")),
        (
            "params",
            Json::obj([("name", Json::str(tool)), ("arguments", args)]),
        ),
    ])
    .to_string();
    let reply = parse(&server.handle(&line).unwrap()).unwrap();
    let result = reply.get("result").unwrap().clone();
    let text = result
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|i| i.last())
        .and_then(|i| i.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();
    match result.get("isError").and_then(|e| e.as_bool()) {
        Some(true) => Err(text),
        _ => Ok(text),
    }
}

fn zx81_server() -> Option<Server> {
    if std::fs::read("roms/zx81.rom").is_err() {
        eprintln!("need roms/zx81.rom; skipping");
        return None;
    }
    let mut server = Server::new();
    let text = call(
        &mut server,
        "set_machine",
        Json::obj([("model", Json::str("zx81"))]),
    )
    .expect("a ZX81 should start");
    assert!(text.contains("ZX81"), "{text}");
    Some(server)
}

/// A ZX81 runs, and its registers and memory can be read.
#[test]
fn a_zx81_runs_and_can_be_read() {
    let Some(mut server) = zx81_server() else {
        return;
    };
    call(
        &mut server,
        "run_frames",
        Json::obj([("frames", Json::num(50))]),
    )
    .unwrap();

    let text = call(&mut server, "registers", Json::obj([])).unwrap();
    assert!(text.contains("PC=$"), "{text}");
    assert!(
        text.contains("line"),
        "the ZX81 counts lines, not T-states in a ULA frame: {text}"
    );

    // The ROM is where a ZX81's ROM is, and it is not all zeros.
    let text = call(
        &mut server,
        "read_memory",
        Json::obj([("address", Json::num(0)), ("length", Json::num(16))]),
    )
    .unwrap();
    assert!(
        !text.contains("00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00"),
        "{text}"
    );
    assert!(
        text.contains("not ASCII"),
        "the ZX81's character set is its own and the dump should say so: {text}"
    );

    let text = call(
        &mut server,
        "disassemble",
        Json::obj([("address", Json::num(0)), ("count", Json::num(4))]),
    )
    .unwrap();
    assert!(text.contains("$0000"), "{text}");
}

/// The tools that are about a Spectrum's hardware say what they are, rather
/// than reporting zeros — an empty answer reads like a finding.
#[test]
fn spectrum_only_tools_say_why_they_are_not_here() {
    let Some(mut server) = zx81_server() else {
        return;
    };
    for tool in ["sound_state", "paging", "watch_routines", "frame_timing"] {
        let why = call(&mut server, tool, Json::obj([]))
            .unwrap_err()
            .to_string();
        assert!(
            why.contains("ZX81 does not have"),
            "{tool} should say why: {why}"
        );
        assert!(
            why.contains("set_machine 48k"),
            "and how to get back: {why}"
        );
    }
}

/// And the session goes back to a Spectrum when asked.
#[test]
fn a_session_can_go_back_to_a_spectrum() {
    let Some(mut server) = zx81_server() else {
        return;
    };
    if std::fs::read("roms/48.rom").is_err() {
        return;
    }
    call(
        &mut server,
        "set_machine",
        Json::obj([("model", Json::str("48k"))]),
    )
    .unwrap();
    assert!(server.session.zx81.is_none(), "the ZX81 is put away");
    let text = call(&mut server, "machine_info", Json::obj([])).unwrap();
    assert!(text.contains("48K"), "{text}");
}
