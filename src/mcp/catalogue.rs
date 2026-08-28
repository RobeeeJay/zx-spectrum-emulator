//! What the server says it can do, in the shape MCP asks for it.
//!
//! Each tool carries a description written for the thing reading it: what it
//! is for, what it costs, and what has to have happened first. A model that
//! calls `routines` before `watch_routines` gets nothing, so the descriptions
//! say so rather than leaving it to be discovered.

use crate::mcp::json::Json;

/// A JSON Schema property.
fn prop(kind: &str, description: &str) -> Json {
    Json::obj([
        ("type", Json::str(kind)),
        ("description", Json::str(description)),
    ])
}

/// An address property: a number, or a string like "$8000".
fn address(description: &str) -> Json {
    Json::obj([
        (
            "type",
            Json::arr(vec![Json::str("integer"), Json::str("string")]),
        ),
        ("description", Json::str(description)),
    ])
}

fn tool<const N: usize>(name: &str, description: &str, properties: [(&str, Json); N]) -> Json {
    schema_tool(name, description, properties, &[])
}

fn schema_tool<const N: usize>(
    name: &str,
    description: &str,
    properties: [(&str, Json); N],
    required: &[&str],
) -> Json {
    Json::obj([
        ("name", Json::str(name)),
        ("description", Json::str(description)),
        (
            "inputSchema",
            Json::obj([
                ("type", Json::str("object")),
                (
                    "properties",
                    Json::Obj(
                        properties
                            .into_iter()
                            .map(|(k, v)| (k.to_string(), v))
                            .collect(),
                    ),
                ),
                (
                    "required",
                    Json::arr(required.iter().map(|r| Json::str(*r)).collect()),
                ),
            ]),
        ),
    ])
}

pub fn tools() -> Json {
    Json::arr(vec![
        tool(
            "machine_info",
            "What machine is running, where it has got to, what is loaded, and whether \
             the observer is on. A good first call.",
            [],
        ),
        schema_tool(
            "set_machine",
            "Switch model and reset. The ROM must be findable: roms/ beside the emulator, \
             or the preferences directory.",
            [("model", prop("string", "48k, 128k, +2a or +3"))],
            &["model"],
        ),
        tool("reset", "Reset the machine, as the power switch would.", []),
        schema_tool(
            "load_tape",
            "Put a .tap, .tzx or .zip in the deck. By default it also types LOAD \"\" and \
             runs the tape to its end with the ROM loader handed over, which takes a \
             second or two of real time rather than the four minutes the tape lasts. \
             Pass autoload: false to load it without starting it.",
            [
                ("path", prop("string", "the tape file")),
                (
                    "autoload",
                    prop("boolean", "type LOAD \"\" and run it (default true)"),
                ),
            ],
            &["path"],
        ),
        schema_tool(
            "load_snapshot",
            "Load a .sna or .z80. The model comes from the file. This is the quickest way \
             into the middle of a game.",
            [("path", prop("string", "the snapshot file"))],
            &["path"],
        ),
        schema_tool(
            "load_recording",
            "Load an .rzx recording: a snapshot plus every byte the machine read from a \
             port afterwards. play_recording then replays somebody else's session, which \
             is how to watch a game being played without playing it.",
            [("path", prop("string", "the recording"))],
            &["path"],
        ),
        tool(
            "play_recording",
            "Play a loaded recording forward. A frame of a recording is a number of opcode \
             fetches rather than a number of T-states, and the interrupt comes at the \
             recording's own frame boundary.",
            [("frames", prop("integer", "how many frames (default 1)"))],
        ),
        tool(
            "tape_blocks",
            "What is on the tape: every block, with the header of a standard one decoded — \
             what it loads and at what address. This is the memory map before there is one. \
             A turbo block is the game's own loader at work.",
            [(
                "limit",
                prop("integer", "how many blocks to list (default 60)"),
            )],
        ),
        tool(
            "loader",
            "Which loader the machine is sitting in, read off the sampling loop it is \
             counting pulses in — Speedlock, Bleepload, Alkatraz and the rest. Nine are \
             recognised, taken off the tapes rather than from a list.",
            [],
        ),
        tool(
            "step",
            "Step instructions, printing each one as it goes. With over: true a CALL or a \
             block instruction is run to completion rather than stepped into.",
            [
                ("count", prop("integer", "how many (default 1)")),
                ("over", prop("boolean", "step over calls (default false)")),
            ],
        ),
        tool(
            "run_frames",
            "Run whole TV frames — 69,888 T-states each on a 48K, 70,908 on a 128K. Stops \
             early on a breakpoint or a watched event.",
            [("frames", prop("integer", "how many (default 1)"))],
        ),
        tool(
            "run_tstates",
            "Run a number of T-states. Use this to stop part-way through a frame: the ULA \
             draws the screen between T-state 14,336 and 57,344 on a 48K, so where in the \
             frame a routine runs decides whether its writes are seen this frame or next.",
            [("tstates", prop("integer", "how many"))],
        ),
        tool(
            "run_until",
            "Run until the program reaches an address. This is the breakpoint: give one \
             address or several, and it stops at whichever comes first.",
            [
                ("address", address("where to stop")),
                (
                    "addresses",
                    Json::obj([
                        ("type", Json::str("array")),
                        ("items", address("an address")),
                        ("description", Json::str("several places to stop")),
                    ]),
                ),
                (
                    "max_frames",
                    prop("integer", "give up after this many frames (default 1000)"),
                ),
            ],
        ),
        tool(
            "watch_events",
            "Stop on what a program does rather than on where it is: a write to the display \
             file, the beeper or the sound chip being touched, the frame interrupt, a call \
             into the ROM, any IN, any OUT. Switch on what you want and pass run: true. \
             This is how to find the drawing routine, the keyboard read or the music player \
             without knowing an address.",
            [
                (
                    "screen",
                    prop("boolean", "a write anywhere in the display file"),
                ),
                (
                    "beeper",
                    prop("boolean", "the speaker or MIC bit of port $FE"),
                ),
                ("ay", prop("boolean", "any access to the sound chip")),
                (
                    "interrupt",
                    prop("boolean", "the frame interrupt being taken"),
                ),
                (
                    "rom",
                    prop("boolean", "code entering the ROM from outside it"),
                ),
                ("port_in", prop("boolean", "any IN")),
                ("port_out", prop("boolean", "any OUT")),
                ("run", prop("boolean", "run until one of them happens")),
                (
                    "max_frames",
                    prop("integer", "how long to wait (default 1000)"),
                ),
            ],
        ),
        schema_tool(
            "press_keys",
            "Press keys, hold them, and let go. Several together is a chord: [\"CAPS SHIFT\", \
             \"1\"]. The ROM scans the keyboard once a frame and wants a key on two scans \
             running, so a key is held for ten frames by default. This is how a game is \
             driven to the part worth looking at, and how to find the input routine: press \
             something and see who reads port $FE.",
            [
                (
                    "keys",
                    Json::obj([
                        (
                            "type",
                            Json::arr(vec![Json::str("array"), Json::str("string")]),
                        ),
                        (
                            "items",
                            prop(
                                "string",
                                "a key: A-Z, 0-9, ENTER, SPACE, CAPS SHIFT, SYMBOL SHIFT",
                            ),
                        ),
                        ("description", Json::str("the keys to hold down together")),
                    ]),
                ),
                (
                    "frames",
                    prop("integer", "how long to hold them (default 10)"),
                ),
                (
                    "then_frames",
                    prop("integer", "frames to run afterwards (default 10)"),
                ),
            ],
            &["keys"],
        ),
        schema_tool(
            "type_text",
            "Type a line: letters, digits and spaces, with ENTER at the end. For anything \
             behind a shift, use press_keys.",
            [
                ("text", prop("string", "what to type")),
                (
                    "enter",
                    prop("boolean", "press ENTER at the end (default true)"),
                ),
                (
                    "frames",
                    prop("integer", "frames a key is held (default 6)"),
                ),
            ],
            &["text"],
        ),
        tool(
            "registers",
            "Every register, the flags spelled out, and where the machine is in the frame.",
            [],
        ),
        schema_tool(
            "read_memory",
            "Read memory as hex, with the printable bytes beside it. Up to 4096 bytes a \
             call. Without a bank this is what the CPU sees through its current paging; \
             with one it is that RAM bank whatever is paged in.",
            [
                ("address", address("where to start")),
                ("length", prop("integer", "how many bytes (default 256)")),
                ("bank", prop("integer", "a 128K RAM bank, 0-7")),
            ],
            &["address"],
        ),
        schema_tool(
            "write_memory",
            "Poke bytes in. For testing a theory — freeze the lives counter, blank a sprite \
             and see what stops moving — not for patching a file: nothing is written to disc.",
            [
                ("address", address("where to write")),
                (
                    "bytes",
                    prop("string", "hex bytes, \"3E 00 32\", or a list of numbers"),
                ),
            ],
            &["address", "bytes"],
        ),
        tool(
            "memory_activity",
            "Where the program keeps things: reads, writes and whether anything was executed, \
             counted per 256-byte page, with a guess at what each page is for. Also whether a \
             back buffer has been detected — a screen built somewhere other than the display \
             file and blitted across. This is the map to read before disassembling anything.",
            [
                ("from", address("lowest address (default $4000)")),
                ("to", address("highest address (default $FFFF)")),
                (
                    "include_quiet",
                    prop("boolean", "list pages nothing has touched"),
                ),
                ("limit", prop("integer", "how many pages (default 40)")),
            ],
        ),
        tool(
            "find_bytes",
            "Where a sequence of bytes appears in memory: a sprite you have the bytes of, a \
             string the game prints, a value you are hunting for.",
            [
                (
                    "bytes",
                    prop("string", "hex bytes, \"3E 00 32\", or a list of numbers"),
                ),
                ("text", prop("string", "ASCII to look for instead")),
                ("value", prop("integer", "a single byte to look for")),
                ("from", address("where to start (default $4000)")),
                ("to", address("where to stop (default $FFFF)")),
                (
                    "limit",
                    prop("integer", "how many hits to list (default 40)"),
                ),
            ],
        ),
        tool(
            "changed_since",
            "What is different in memory from a state saved earlier. This is how a variable \
             is found: save the machine, lose a life, ask what changed, and the counter is \
             among the handful of addresses that come back. Narrow it with value. The \
             display file is left out unless asked for, since it changes every frame and \
             says nothing.",
            [
                (
                    "name",
                    prop(
                        "string",
                        "the saved state to compare with (default \"last\")",
                    ),
                ),
                (
                    "value",
                    prop("integer", "only addresses that now hold this byte"),
                ),
                ("from", address("lowest address (default $4000)")),
                ("to", address("highest address (default $FFFF)")),
                (
                    "include_screen",
                    prop("boolean", "include $4000-$5AFF (default false)"),
                ),
                ("limit", prop("integer", "how many to list (default 60)")),
            ],
        ),
        tool(
            "save_state",
            "Take a snapshot of the whole machine and keep it under a name, so an experiment \
             can be undone. With a path it is also written as a .sna file.",
            [
                ("name", prop("string", "what to call it (default \"last\")")),
                ("path", prop("string", "also write it here as .sna")),
            ],
        ),
        tool(
            "restore_state",
            "Put a saved machine back, by name or from a .sna or .z80 file.",
            [
                ("name", prop("string", "a name given to save_state")),
                ("path", prop("string", "a snapshot file instead")),
            ],
        ),
        tool(
            "screen",
            "What is on the screen: a PNG you can look at, a sketch of the 32x24 character \
             cells for when you cannot, and what the attributes are set to. This is how to \
             check whether a change did what you thought — did the sprite disappear, is the \
             score where you think it is.",
            [
                (
                    "image",
                    prop("boolean", "send the picture as well (default true)"),
                ),
                (
                    "border",
                    prop("boolean", "include the border (default false)"),
                ),
                (
                    "path",
                    prop("string", "write the PNG here instead of sending it"),
                ),
            ],
        ),
        schema_tool(
            "graphics",
            "Read memory as characters and sprites — eight bytes a character, one bit a \
             pixel — and draw them. Point it at a candidate address and see whether letters, \
             a sprite or rubbish comes out. This is how graphics are found.",
            [
                ("address", address("where the graphics might start")),
                ("count", prop("integer", "how many characters (default 16)")),
                (
                    "across",
                    prop("integer", "how many to a row, 1-32 (default 8)"),
                ),
                (
                    "image",
                    prop("boolean", "send a picture as well as the text"),
                ),
            ],
            &["address"],
        ),
        tool(
            "disassemble",
            "Disassemble, with your labels and comments against the lines that have them, \
             and a note of whether each address has been executed or only read while \
             watching — which is how code is told from data.",
            [
                ("address", address("where to start (default PC)")),
                (
                    "count",
                    prop("integer", "how many instructions (default 32)"),
                ),
                (
                    "comments",
                    prop("boolean", "include labels and comments (default true)"),
                ),
            ],
        ),
        tool(
            "watch_routines",
            "Switch the observer on. It attributes every write, read, port access and loop \
             to the routine making it, which is what routines, routine, call_graph, code_map \
             and autodoc read. Run the machine afterwards — a few hundred frames of a game \
             is plenty. It costs a branch on every access, so switch it off when finished.",
            [
                ("enabled", prop("boolean", "on or off (default on)")),
                ("clear", prop("boolean", "forget what was seen so far")),
            ],
        ),
        tool(
            "routines",
            "Every routine seen called, with how often, how much work it did, what it wrote \
             to the screen and the attributes, what it read, and how big it is. Sorted by \
             work done unless told otherwise.",
            [
                ("sort", prop("string", "work, calls, writes or address")),
                ("limit", prop("integer", "how many to list (default 50)")),
            ],
        ),
        schema_tool(
            "routine",
            "One routine in full: its extent, its writes and reads per call, its ports, its \
             loops and how many times round they went, where it exits, and what was in the \
             registers when it was entered.",
            [("address", address("the routine's entry point"))],
            &["address"],
        ),
        tool(
            "frame_timing",
            "Where the beam is, and when in the frame a watched routine ran. On this machine \
             when is half the question: the ULA puts the picture out as it goes, so a write \
             above the beam is seen this frame and one below it waits for the next — which \
             is what a flickering sprite is.",
            [(
                "address",
                address("a routine's entry point, to ask about that one"),
            )],
        ),
        tool(
            "call_graph",
            "Who called whom, and how often. A line back to something already reached is a \
             loop in the program.",
            [("limit", prop("integer", "how many edges (default 60)"))],
        ),
        tool(
            "code_map",
            "Which addresses were executed and which were only read: what is code and what \
             is data, as far as the machine has got. Blocks in $4000-$5AFF are the screen \
             itself.",
            [(
                "min_length",
                prop("integer", "shortest data block to report (default 8)"),
            )],
        ),
        tool(
            "load_symbols",
            "Read names for addresses out of a symbol file — a ROM disassembly turned into \
             one by tools/rom-symbols.py, or a game's by tools/skool-symbols.py — and build \
             the table of routines recognisable by their first bytes. With no path it reads \
             the files for this machine from the preferences directory. Names loaded this \
             way appear in disassemble, and a CALL to a named address says which.",
            [(
                "path",
                prop("string", "a symbol file; the machine's own by default"),
            )],
        ),
        tool(
            "symbols",
            "The names that came from a symbol file, in an address range.",
            [
                ("from", address("lowest address")),
                ("to", address("highest address")),
            ],
        ),
        schema_tool(
            "identify",
            "What the code at an address is, if its first twelve bytes are a routine that is \
             known. Games copy ROM routines into RAM and the bytes hash the same wherever \
             they land, so a copy is named after the original and said to be one.",
            [("address", address("where the code starts"))],
            &["address"],
        ),
        schema_tool(
            "xrefs",
            "Everything that refers to an address, both ways round: which routines were \
             watched calling it, which hammered it, which read its page, and which \
             instruction in memory names it. The watched answers are facts about the run; \
             the instructions are a search, and some of what it finds will be data that \
             happens to look like code. This is the question to ask when naming a variable.",
            [
                ("address", address("the address to look for")),
                (
                    "search_from",
                    address("where the search starts (default $4000)"),
                ),
                ("search_to", address("where it ends (default $FFFF)")),
            ],
            &["address"],
        ),
        tool(
            "autodoc",
            "Guesses at what routines do, from what they touch — ports, screen ranges, ROM \
             calls, block moves — and from what they were measured doing. It hedges where \
             the evidence is thin and leaves code with no tell unnamed. Guesses, not answers.",
            [(
                "entries",
                Json::obj([
                    ("type", Json::str("array")),
                    ("items", address("a routine entry point")),
                    (
                        "description",
                        Json::str("which routines to look at; the busiest watched ones by default"),
                    ),
                ]),
            )],
        ),
        schema_tool(
            "set_comment",
            "Write a label, a comment, or both against an address. What is typed here is \
             yours: a later guess never overwrites it.",
            [
                ("address", address("the address")),
                ("label", prop("string", "a name for it")),
                ("comment", prop("string", "what it does")),
            ],
            &["address"],
        ),
        tool(
            "comments",
            "Read back the labels and comments, with the guesses marked as guesses.",
            [
                ("from", address("lowest address (default $0000)")),
                ("to", address("highest address (default $FFFF)")),
            ],
        ),
        tool(
            "save_comments",
            "Write the notes to their file beside the tape or snapshot — plain text, one \
             line an address, readable and diffable without the emulator.",
            [],
        ),
    ])
}
