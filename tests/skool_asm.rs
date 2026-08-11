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
            let trimmed = line.trim_start_matches(['b', 'c', 'g', 'i', 's', 't', 'u', 'w', '*', ' ']);
            let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
            digits.len() >= 4 && trimmed.len() > digits.len()
        })
        .count();

    let out = std::process::Command::new("python3")
        .args(["tools/skool-to-asm.py", "skoolkit/mm.skool", "--hex"])
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
