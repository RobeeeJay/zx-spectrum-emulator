#!/usr/bin/env python3
"""Ask a local model what each routine in a trace is for, and score the answers.

Takes the JSONL that `trace` writes — a routine's listing, what it was measured
doing, its callers and callees, and a character cell it actually drew — and
asks a model for one sentence about each. The answers are scored by the same
harness the rules are scored by, so the two numbers can be compared.

    ollama pull qwen2.5-coder:7b
    ./target/release/trace recordings/manic.rzx > manic.jsonl
    python3 tools/ask-model.py manic.jsonl --model qwen2.5-coder:7b

Three ways of asking are supported, because the interesting question is not
how well a model does but how much of any success is the model and how much is
the instrumentation:

    --prompt full          the listing and the measurements (the default)
    --prompt listing       the listing alone
    --prompt measured      the measurements alone

A model given only the measurements is being asked to phrase a conclusion
something else reached. A model given only the listing is being asked to read
Z80. The gap between those two is the answer to whether any of this needs a
model at all.
"""

import argparse
import json
import sys
import urllib.error
import urllib.request

SYSTEM = """\
You are reading a routine from a ZX Spectrum game, disassembled from a real \
run of the program. Say in one short sentence what the routine is for.

Rules:
- One sentence. No preamble, no markdown, no restating the question.
- Say what it does, not how. "Draws the player sprite" not "loads HL and writes".
- Say what the evidence supports and no more. "Writes a block of bytes into \
  the display file" is a good answer when you cannot tell what is being drawn.
- Only answer "unknown" if there is genuinely nothing to say.

Hedge the wording where the evidence is thin — "appears to", "probably" — \
rather than declining. An answer that describes what the routine measurably \
does is useful even when its purpose in the game is not clear.\
"""


def prompt_for(episode, style):
    """What to show the model about one routine."""
    parts = [f"Routine at ${episode['address']}."]

    if style in ("full", "measured"):
        writes = episode["writes"]
        facts = [
            f"Called {episode['calls']} times, in {episode['frames_seen']} of "
            f"{episode['of_frames']} frames.",
            f"Wrote {writes['screen']} bytes to the display file, "
            f"{writes['attrs']} to the attributes, {writes['other']} elsewhere.",
        ]
        if episode["longest_loop"]:
            facts.append(f"Its longest loop went round {episode['longest_loop']} times.")
        if episode["ports_in"]:
            facts.append(f"Read ports: {', '.join('$' + p for p in episode['ports_in'])}.")
        if episode["ports_out"]:
            facts.append(f"Wrote ports: {', '.join('$' + p for p in episode['ports_out'])}.")
        low, high = episode["entry_hl"]
        if low != high:
            facts.append(f"HL held ${low}..${high} on entry.")
        if episode["callers"]:
            facts.append(f"Called from: {', '.join('$' + c for c in episode['callers'][:6])}.")
        if episode["callees"]:
            facts.append(f"Calls: {', '.join('$' + c for c in episode['callees'][:6])}.")
        parts.append("What it was measured doing:\n" + "\n".join("- " + f for f in facts))

        if episode.get("drew"):
            # The picture it actually put on screen, which no amount of reading
            # the code would give.
            parts.append(
                "A character cell it drew (# is a lit pixel):\n" + "\n".join(episode["drew"])
            )

    if style in ("full", "listing"):
        parts.append("Its code:\n" + "\n".join(episode["listing"]))

    return "\n\n".join(parts)


def ask(model, system, prompt, timeout, host="http://127.0.0.1:11434"):
    """One question to the model.

    Through the HTTP API rather than the `ollama run` command: the command
    draws a spinner into its own output, which ends up in the answer, and it
    lets the model fall out of memory between questions. `keep_alive` holds it
    there, which is the difference between a few seconds a routine and most of
    a minute.
    """
    body = json.dumps({
        "model": model,
        "system": system,
        "prompt": prompt,
        "stream": False,
        "keep_alive": "10m",
        "options": {"temperature": 0, "num_predict": 80},
    }).encode()
    request = urllib.request.Request(
        f"{host}/api/generate", data=body, headers={"Content-Type": "application/json"}
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout) as reply:
            said = json.load(reply).get("response", "")
    except (urllib.error.URLError, TimeoutError, json.JSONDecodeError) as e:
        print(f"  ({e})", file=sys.stderr)
        return ""
    return " ".join(said.strip().split())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("episodes")
    parser.add_argument("--model", default="qwen2.5-coder:7b")
    parser.add_argument("--prompt", default="full", choices=["full", "listing", "measured"])
    parser.add_argument("--limit", type=int, default=0, help="only this many routines")
    parser.add_argument("--timeout", type=int, default=120)
    parser.add_argument("--out", help="write the answers as JSONL")
    args = parser.parse_args()

    episodes = [json.loads(line) for line in open(args.episodes, encoding="utf-8")]
    if args.limit:
        episodes = episodes[: args.limit]

    answers = []
    for n, episode in enumerate(episodes, 1):
        said = ask(args.model, SYSTEM, prompt_for(episode, args.prompt), args.timeout)
        answers.append({"address": episode["address"], "said": said})
        print(f"[{n}/{len(episodes)}] ${episode['address']}  {said[:96]}", file=sys.stderr)

    if args.out:
        with open(args.out, "w", encoding="utf-8") as f:
            for answer in answers:
                f.write(json.dumps(answer) + "\n")

    unknown = sum(1 for a in answers if a["said"].strip().lower().startswith("unknown"))
    print(
        f"\n{len(answers)} answered, {unknown} declined to guess "
        f"({unknown * 100 // max(len(answers), 1)}%)",
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()
