#!/usr/bin/env python3
"""Score AutoDoc's rules against annotations somebody else wrote.

Reads the CSV `corpus` produces — features, what the rules made of each
routine, and the description from a disassembly — and prints how often the two
agree.

    cargo build --release --bin corpus
    ./target/release/corpus roms/48.rom symbols-48.txt > rom48.csv
    python3 tools/score-autodoc.py rom48.csv

Three numbers matter, and they are reported separately because they mean
different things:

* **silence** — routines the rules decline to name. Intended behaviour: a rule
  that always has an answer would be worse than none.
* **agreement** — where the rules name something *and* the description says
  what it is, how often they mean the same thing.
* **coverage of the truth** — how many descriptions this script can place into
  a category at all. Everything outside that is unscored, and a low number here
  means the score below is measuring a small corner rather than the corpus.

The mapping from free text to category is keyword matching, which is a proxy
and not a good one: "Print a character" and "The 'print output' routine" are
the same thing to a reader and different strings to this. It is deterministic
and inspectable, which is the reason to start here rather than with something
cleverer — the number it gives can be argued with.
"""

import csv
import re
import sys
from collections import Counter, defaultdict

# What a routine can be. Deliberately coarse: these are the distinctions the
# emulator can act on, not a taxonomy of everything a program might do.
CATEGORIES = {
    "clear_screen": ["clear the screen", "clear screen", "cls", "clear the display"],
    "draw": ["draw", "plot", "sprite", "print a character", "display the", "put the",
             "render", "blit", "graphic", "pixel"],
    "colour": ["attribute", "colour", "color", "ink", "paper", "border", "flash", "bright"],
    "print_text": ["print", "message", "string", "text", "character set", "font", "caption"],
    "read_keys": ["keyboard", "key scan", "keypress", "read the keys", "scanning the keyboard"],
    "joystick": ["joystick", "kempston", "sinclair interface"],
    "sound": ["beep", "sound", "note", "tune", "music", "click", "ay ", "psg"],
    "tape": ["tape", "load a", "loading", "save", "header", "cassette", "verify"],
    "decompress": ["decompress", "uncompress", "unpack", "depack", "expand"],
    "score": ["score", "points", "high score", "bonus"],
    "lives": ["life", "lives", "death", "died", "killed"],
    "collision": ["collision", "collide", "hit", "touch"],
    "protection": ["protection", "copy protect", "checksum", "anti-"],
    "copy": ["copy", "move a block", "block move", "transfer"],
    "maths": ["arithmetic", "multiply", "divide", "calculator", "floating point", "add ", "subtract"],
    "variables": ["variable", "flag", "counter", "pointer", "table of", "store the"],
    "interrupt": ["interrupt", "im 2", "frame counter", "frames"],
    "logic": ["main loop", "game loop", "update the", "move the", "control", "turn"],
}

# What the rules call things, in the same terms.
RULE_TO_CATEGORY = {
    "clear_screen": "clear_screen",
    "blit_screen": "draw",
    "copy_to_screen": "draw",
    "draw_sprite": "draw",
    "draw_to_screen": "draw",
    "draw_with_colour": "draw",
    "draw_panel": "draw",
    "set_colours": "colour",
    "colour_screen": "colour",
    "print_text": "print_text",
    "read_keys": "read_keys",
    "read_joystick": "joystick",
    "play_sound": "sound",
    "load_from_tape": "tape",
    "save_to_tape": "tape",
    "decompress": "decompress",
    "pack_data": "decompress",
    "update_score": "score",
    "maybe_collision": "collision",
    "maybe_protection": "protection",
    "copy_block": "copy",
    "update_variable": "variables",
    "game_state": "logic",
    "game_logic": "logic",
    "game_turn": "logic",
    # Not a claim.
    "routine": None,
}


def category_of(description):
    """Which category a description falls into, or None if it is not clear.

    A description matching several is ambiguous rather than the first one
    listed: "Print the score" is both text and score, and counting it as
    whichever came first in a dictionary would flatter or punish the rules
    arbitrarily.
    """
    text = description.lower()
    hits = {
        name
        for name, words in CATEGORIES.items()
        if any(re.search(r"\b" + re.escape(w), text) for w in words)
    }
    return hits or None


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)

    rows = []
    for path in sys.argv[1:]:
        with open(path, newline="", encoding="utf-8") as f:
            rows.extend(csv.DictReader(f))

    silent = 0
    scored = 0
    agree = 0
    unmapped = 0
    described = 0
    confusion = defaultdict(Counter)

    for row in rows:
        claim = RULE_TO_CATEGORY.get(row["rule_label"], row["rule_label"])
        description = row.get("description", "").strip()
        if claim is None:
            silent += 1
        if not description:
            continue
        described += 1
        truth = category_of(description)
        if truth is None:
            unmapped += 1
            continue
        if claim is None:
            continue
        scored += 1
        if claim in truth:
            agree += 1
        else:
            confusion[claim][sorted(truth)[0]] += 1

    total = len(rows)
    print(f"{total} routines, {described} with a description")
    print(f"  silence          {silent:5} ({pct(silent, total)}) no claim made")
    print(f"  unmapped truth   {unmapped:5} ({pct(unmapped, described)}) description "
          f"fits no category here")
    print(f"  scored           {scored:5} rules claimed something checkable")
    print(f"  AGREEMENT        {agree:5} ({pct(agree, scored)})")

    if confusion:
        print("\nwhere they differ, most often first:")
        worst = sorted(
            ((claim, truth, n) for claim, counts in confusion.items()
             for truth, n in counts.items()),
            key=lambda item: -item[2],
        )
        for claim, truth, n in worst[:12]:
            print(f"  {n:4}  rules said {claim:14} description says {truth}")


def pct(part, whole):
    return f"{part * 100 // whole if whole else 0}%"


if __name__ == "__main__":
    main()
