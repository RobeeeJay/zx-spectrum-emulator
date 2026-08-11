#!/usr/bin/env python3
"""Write out the prompt describing one turn of a program's loop.

Separated from the sending so it can be read, edited and tried by hand:

    ./target/release/trace recordings/manic.rzx --frames 4000 --loops \
        > /dev/null 2>&1                      # TURN_JSON=/tmp/turn writes the turn
    ./target/release/trace recordings/manic.rzx --frames 1200 > /tmp/manic.jsonl
    python3 tools/loop-prompt.py /tmp/turn.1.json /tmp/manic.jsonl > prompt.txt

Then send it however you like:

    python3 tools/send-prompt.py prompt.txt --model qwen2.5-coder:7b
"""

import json
import sys

INSTRUCTION = """\
Explain what this loop is doing, in four or five sentences: what the program \
appears to be doing each time round, in what order, and which routines look \
like they belong together. Where the evidence does not say, say so."""


def summarise(address, episode):
    """One routine's measurements, on a line."""
    inclusive = episode.get("inclusive", {})
    calls = max(episode.get("calls", 1), 1)
    work = sum(inclusive.get(k, 0) for k in ("screen", "attrs", "other")) // calls
    span = episode.get("wrote_between") or ["-", "-"]
    return (f"{work} bytes a call including what it calls, "
            f"writes ${span[0]}..${span[1]}, longest loop {episode.get('longest_loop', 0)}")


def build(turn, episodes, listing_lines=60):
    # The call tree, with runs of the same routine collapsed: twelve identical
    # lines say no more than "12x" and crowd out the code.
    lines = []
    previous = None
    repeats = 0
    for call in turn["calls"] + [None]:
        here = (call["depth"], call["routine"].replace("$", "")) if call else None
        if here == previous:
            repeats += 1
            continue
        if previous is not None:
            depth, address = previous
            times = f"  x{repeats + 1}" if repeats else ""
            lines.append("  " * (depth - 1) + f"{address}{times}")
        previous, repeats = here, 0

    # Every routine in the turn, once, with its code. The listing has the loops
    # marked and how many times each was measured going round.
    seen = []
    for call in turn["calls"]:
        address = call["routine"].replace("$", "")
        if address not in seen:
            seen.append(address)

    code = []
    for address in seen:
        episode = episodes.get(address)
        if not episode:
            continue
        code.append(f"--- ${address}: {summarise(address, episode)}")
        flow = episode.get("flow") or episode.get("listing") or []
        code.extend(flow[:listing_lines])
        if len(flow) > listing_lines:
            code.append(f"    ... {len(flow) - listing_lines} more instructions")
        if episode.get("call_sites"):
            code.append("  how its callers set it up:")
            code.extend("  " + line for line in episode["call_sites"][:6])
        code.append("")

    return f"""A ZX Spectrum game, watched while somebody played it. Nobody has annotated this game; everything below was measured by the emulator.

The program goes round this loop every {turn['frames_per_turn']} frames. Note that is not once per frame — it takes four frames and a bit over each turn. Writes to $4000-$57FF are the screen, but this game writes mostly elsewhere, which on this machine usually means it is drawing into a buffer and copying it over later.

One complete turn, in the order the calls happened, indented by call depth:

{chr(10).join(lines)}

The code of each routine in that turn, with its loops marked and how many times each was measured going round:

{chr(10).join(code)}
{INSTRUCTION}"""


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    turn = json.load(open(sys.argv[1], encoding="utf-8"))
    episodes = {e["address"]: e for e in (json.loads(l) for l in open(sys.argv[2], encoding="utf-8"))}
    print(build(turn, episodes))


if __name__ == "__main__":
    main()
