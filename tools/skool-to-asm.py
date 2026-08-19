#!/usr/bin/env python3
"""Turn a SkoolKit disassembly into an assembly listing that reads as names.

    python3 tools/skool-to-asm.py skoolkit/mm.skool > mm.asm
    python3 tools/skool-to-asm.py skoolkit/mm.skool --hex > mm.asm
    python3 tools/skool-to-asm.py skoolkit/mm.skool --with-data > mm.asm

Data is left out unless asked for. Most of a game is graphics, tables and
messages, and a thousand lines of DEFB say nothing about what the program
does; what is wanted is usually the code and what somebody wrote about it. The
description above a block of data goes with it, since a heading over nothing
is worse than neither.

SkoolKit ships `skool2asm.py`, which produces a listing that assembles back
into the original bytes and is the right tool if that is what you want. This
does something else: it substitutes the labels into the operands, so

    34700 CALL 36266

comes out as

    CALL DRAWHG

which is the difference between a listing you can assemble and one you can
read. Addresses with no label of their own keep their number.

Comments are kept, with SkoolKit's own markup taken out: #R36266 becomes the
label it points at, #REGhl becomes HL, and the {braces} that group a comment
across several instructions are dropped.
"""

import argparse
import re
import sys

# `c34252 XOR A         ; comment` — the letter says what the block holds.
ENTRY = re.compile(r"^([bcgistuw])(\$?[0-9A-Fa-f]+)\s*(.*)$")
# ` 34253 LD (33799),A  ; comment` — a line within an entry. A `*` in the
# first column marks one that is jumped to from elsewhere, and dropping those
# lines loses an instruction from the middle of a routine.
WITHIN = re.compile(r"^[ *]\s*(\$?[0-9A-Fa-f]+)\s+(.*)$")
# A comment carried on to the next line, indented, with no address.
CONTINUED = re.compile(r"^\s+;\s?(.*)$")
LABEL = re.compile(r"^@label=(\w+)")
COMMENT = re.compile(r"^;\s?(.*)$")
DIRECTIVE = re.compile(r"^@")
# Which letter begins an entry says what it holds: c is code, and the rest is
# data of one sort or another.
CODE_BLOCK = "c"
# DEFB, DEFW, DEFM, DEFS — data sitting inside a code block.
DEFINITION = re.compile(r"^\s*DEF[BWMS]\b", re.I)

# `#R36266` points at an address. Written to take decimal or an explicit $
# hex, and not to swallow the E of `#REGa` as a hex digit, which it did.
MACRO_ADDRESS = re.compile(r"#R(\$[0-9A-Fa-f]{1,4}|\d+)\b")
MACRO_REG = re.compile(r"#REG([a-z']+)", re.I)
MACRO_OTHER = re.compile(r"#[A-Z]+(\([^)]*\))?")
# A number in an operand: what might be an address worth naming.
OPERAND_NUMBER = re.compile(r"(?<![\w$])(\d{3,5})(?![\w])")


def block_letter(line):
    """The letter that begins an entry, if this line begins one."""
    entry = ENTRY.match(line)
    return entry.group(1) if entry else None


def parts(line):
    """An address and what follows it, from either kind of line.

    An entry line puts the block letter first — `c34252 XOR A` — so its
    address is the second group; a line within an entry has no letter and the
    address is the first. Reading both the same way parses the letter as an
    address, which is where this went wrong.
    """
    entry = ENTRY.match(line)
    if entry:
        return entry.group(2), entry.group(3)
    within = WITHIN.match(line)
    if within:
        return within.group(1), within.group(2)
    return None, None


def address_of(text):
    """`$8000` is hex; `32768` is decimal."""
    text = text.strip()
    if text.startswith("$"):
        return int(text[1:], 16)
    return int(text)


def labels(lines):
    """First pass: every address that has a name."""
    found = {}
    pending = None
    for line in lines:
        found_label = LABEL.match(line)
        if found_label:
            pending = found_label.group(1)
            continue
        if DIRECTIVE.match(line) or COMMENT.match(line):
            continue
        where, _ = parts(line)
        if where and pending:
            found[address_of(where)] = pending
            pending = None
    return found


def tidy(comment, names, hex_mode):
    """A comment with SkoolKit's markup taken out."""
    def named(match):
        address = address_of(match.group(1))
        return names.get(address, show(address, hex_mode))

    # Registers first: #REGa must not be read as an address.
    comment = MACRO_REG.sub(lambda m: m.group(1).upper(), comment)
    comment = MACRO_ADDRESS.sub(named, comment)
    comment = MACRO_OTHER.sub("", comment)
    return comment.replace("{", "").replace("}", "").strip()


def show(address, hex_mode):
    return f"${address:04X}" if hex_mode else str(address)


def with_labels(operand, names, hex_mode):
    """Put the names into an operand: CALL 36266 -> CALL DRAWHG."""
    def swap(match):
        address = int(match.group(1))
        # Only what is known to be an address is rewritten. A number that is
        # nobody's address is a count or a byte, and turning 255 into $00FF
        # says something false about it.
        if address in names:
            return names[address]
        return match.group(1)

    return OPERAND_NUMBER.sub(swap, operand)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("skool")
    parser.add_argument("--hex", action="store_true", help="addresses as $XXXX")
    parser.add_argument(
        "--with-data",
        action="store_true",
        help="keep the DEFB/DEFW/DEFM blocks and their descriptions",
    )
    args = parser.parse_args()

    lines = open(args.skool, encoding="utf-8", errors="replace").read().splitlines()
    names = labels(lines)
    print(f"; {len(names)} labels, from {args.skool}")
    print("; Generated by tools/skool-to-asm.py — a listing to read, not to assemble.")
    print()

    block = []
    pending_label = None
    # Whether the entry being read is code. Data blocks are skipped whole,
    # description and all, unless they were asked for.
    in_code = True
    for line in lines:
        letter = block_letter(line)
        if letter is not None:
            in_code = letter == CODE_BLOCK
        skipping = not args.with_data and not in_code

        comment = COMMENT.match(line)
        if comment:
            # Held until it is known what it describes: a heading belongs to
            # the entry under it, and goes with it when that is thrown away.
            block.append(tidy(comment.group(1), names, args.hex))
            continue

        # A comment carried on from the instruction above it.
        carried = CONTINUED.match(line)
        if carried and not parts(line)[0] and skipping:
            continue
        if carried and not parts(line)[0]:
            text = tidy(carried.group(1), names, args.hex)
            if text:
                print(f"        {'':<28} ;   {text}")
            continue

        found_label = LABEL.match(line)
        if found_label:
            pending_label = found_label.group(1)
            continue
        if DIRECTIVE.match(line):
            continue

        where, rest = parts(line)
        if where is None:
            if not line.strip():
                block = []
            continue
        if skipping:
            block = []
            pending_label = None
            continue

        code_text = rest.partition(";")[0]
        if not args.with_data and DEFINITION.match(code_text):
            # Data written inside a code block: a table of bytes between two
            # routines is still data.
            block = []
            continue

        if block:
            # A routine's description, as a banner above it.
            print(";", "-" * 68)
            for text in block:
                print(f"; {text}" if text else ";")
            print(";", "-" * 68)
            block = []

        address = address_of(where)
        code, _, comment = rest.partition(";")
        code = code.strip()
        comment = tidy(comment, names, args.hex)

        if pending_label:
            print(f"{pending_label}:")
            pending_label = None

        mnemonic, _, operand = code.partition(" ")
        operand = with_labels(operand.strip(), names, args.hex)
        text = f"{mnemonic:<6}{operand}".rstrip()
        where = show(address, args.hex)
        if comment:
            print(f"        {text:<28} ; {comment}      [{where}]")
        else:
            print(f"        {text:<28} ;                              [{where}]".rstrip())


if __name__ == "__main__":
    main()
