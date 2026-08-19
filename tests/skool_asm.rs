//! Turning a SkoolKit disassembly into a listing that reads as names.
//!
//! The converter is a script, so what is checked here is the thing that went
//! wrong twice while writing it: lines of the source being silently dropped.

/// Every addressed line of the skool file appears in the listing.
///
/// Two kinds went missing on the way. A `*` in the first column marks an
/// instruction that is jumped to from elsewhere, and those were being skipped
/// — an instruction lost from the middle of a routine, with nothing to say so.
/// An indented `;` carries a comment on from the line above, and dropping
/// those truncated the comment rather than the code.
#[test]
fn no_line_of_the_disassembly_is_lost() {
    let skool = std::path::Path::new("skoolkit/mm.skool");
    if !skool.exists() {
        return;
    }
    let source = std::fs::read_to_string(skool).unwrap();
    let addressed = source
        .lines()
        .filter(|line| {
            let trimmed =
                line.trim_start_matches(['b', 'c', 'g', 'i', 's', 't', 'u', 'w', '*', ' ']);
            let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
            digits.len() >= 4 && trimmed.len() > digits.len()
        })
        .count();

    let out = std::process::Command::new("python3")
        .args([
            "tools/skool-to-asm.py",
            "skoolkit/mm.skool",
            "--hex",
            // Everything, since this is counting what survived the trip.
            "--with-data",
        ])
        .output()
        .expect("the converter should run");
    let listing = String::from_utf8_lossy(&out.stdout);
    let kept = listing.lines().filter(|line| line.contains("[$")).count();

    assert!(
        kept >= addressed,
        "{addressed} addressed lines went in and {kept} came out: {} lost",
        addressed - kept
    );
}

/// Data is left out by default, and its description goes with it.
///
/// Most of a game is graphics, tables and messages. A thousand lines of DEFB
/// say nothing about what the program does, and a heading left standing over
/// data that has been removed is worse than no heading at all.
#[test]
fn data_is_left_out_but_no_code_is() {
    let skool = std::path::Path::new("skoolkit/mm.skool");
    if !skool.exists() {
        return;
    }
    let run = |extra: &[&str]| {
        let mut args = vec!["tools/skool-to-asm.py", "skoolkit/mm.skool", "--hex"];
        args.extend_from_slice(extra);
        let out = std::process::Command::new("python3")
            .args(&args)
            .output()
            .expect("the converter should run");
        String::from_utf8_lossy(&out.stdout).to_string()
    };

    let code_only = run(&[]);
    let everything = run(&["--with-data"]);

    assert!(
        !code_only.contains("DEFB") && !code_only.contains("DEFW"),
        "data was left in"
    );
    assert!(
        everything.contains("DEFB"),
        "and --with-data should keep it"
    );
    assert!(
        code_only.lines().count() * 2 < everything.lines().count(),
        "stripping the data should take most of the file: {} against {}",
        code_only.lines().count(),
        everything.lines().count()
    );

    // Every instruction of every code block is still there. Losing code while
    // dropping data is the failure that would matter.
    let source = std::fs::read_to_string(skool).unwrap();
    let mut in_code = false;
    let mut instructions = 0;
    for line in source.lines() {
        if let Some(letter) = line.chars().next() {
            if "bcgistuw".contains(letter) && line[1..].starts_with(|c: char| c.is_ascii_digit()) {
                in_code = letter == 'c';
            }
        }
        let body = line.trim_start_matches(['b', 'c', 'g', 'i', 's', 't', 'u', 'w', '*', ' ']);
        let digits: String = body.chars().take_while(|c| c.is_ascii_digit()).collect();
        let is_instruction = digits.len() >= 4 && body.len() > digits.len();
        let is_data = body[digits.len()..]
            .trim_start()
            .to_uppercase()
            .starts_with("DEF");
        if in_code && is_instruction && !is_data {
            instructions += 1;
        }
    }
    let kept = code_only.matches("[$").count();
    assert_eq!(
        kept, instructions,
        "{instructions} instructions of code went in and {kept} came out"
    );
}
