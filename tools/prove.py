#!/usr/bin/env python3
"""Have a model make claims, then make the emulator test them.

The model is asked what a routine does and made to cite a measurement that
exists. Then the claim is put to the machine: the routine is patched out, the
same recorded frames are played again, and the screen is compared. A routine
said to draw, which can be removed without anything changing, was not drawing.

    python3 tools/prove.py manic.jsonl recordings/manic.rzx

Nothing here trusts the model. It proposes; the emulator disposes.
"""

import argparse, json, subprocess, sys
sys.path.insert(0, "tools")
import importlib.util

spec = importlib.util.spec_from_file_location("ask", "tools/checked-ask.py")
ask_module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ask_module)

# What the screen can settle. Any claim can be put to it: a routine said to
# keep a counter, which cannot be removed without half the picture going with
# it, was not keeping a counter.
DRAWING = {"draws"}
QUIET = {"keeps a counter", "checks a condition", "reads input", "makes a noise"}


def cells_changed(recording, address, frames):
    """How much of the screen stops happening when the routine is taken out."""
    done = subprocess.run(
        ["./target/release/verify", recording, address, "--frames", str(frames)],
        capture_output=True, text=True, timeout=900,
    )
    for line in done.stdout.splitlines():
        if "character cells" in line:
            # "  72 of 6912 bytes ... across 13 character cells"
            return int(line.split("across")[1].split()[0])
    return None


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("episodes")
    parser.add_argument("recording")
    parser.add_argument("--model", default="qwen2.5-coder:7b")
    parser.add_argument("--limit", type=int, default=8)
    parser.add_argument("--frames", type=int, default=120)
    args = parser.parse_args()

    episodes = [json.loads(l) for l in open(args.episodes, encoding="utf-8")][: args.limit]
    held = refuted = untested = 0

    for episode in episodes:
        answer = None
        prompt = ask_module.described(episode)
        for _ in range(2):
            said = ask_module.ask(args.model, prompt)
            start, end = said.find("{"), said.rfind("}")
            if start < 0:
                continue
            try:
                candidate = json.loads(said[start : end + 1])
            except json.JSONDecodeError:
                continue
            if candidate.get("does") == "no evidence":
                answer = candidate
                break
            actual = ask_module.field_of(episode, candidate.get("field") or "")
            if actual is None or actual != candidate.get("value"):
                prompt = (f"{ask_module.described(episode)}\n\nThat citation was wrong: "
                          f"{candidate.get('field')} is {actual}. Quote a real one.")
                continue
            answer = candidate
            break

        address = episode["address"]
        if not answer or answer.get("does") == "no evidence":
            print(f"${address}  no claim made")
            untested += 1
            continue

        claim = answer.get("does")
        name = answer.get("name", "?")
        cells = cells_changed(args.recording, address, args.frames)
        if cells is None:
            untested += 1
            continue

        draws = claim in DRAWING
        picture_depends = cells > 0
        if draws and picture_depends:
            held += 1
            verdict = f"HOLDS — {cells} cells stop appearing without it"
        elif draws and not picture_depends:
            refuted += 1
            verdict = "REFUTED — the picture is the same without it"
        elif not draws and picture_depends and claim in QUIET:
            refuted += 1
            verdict = f"REFUTED — {cells} cells depend on it, so it is not just that"
        elif not draws and picture_depends:
            held += 1
            verdict = f"consistent — {cells} cells depend on it"
        else:
            held += 1
            verdict = "consistent — the picture does not depend on it"
        print(f"${address}  {name:22} {claim:18} {verdict}")

    print(f"\n{held} claims held, {refuted} refuted by the machine, {untested} untested",
          file=sys.stderr)


if __name__ == "__main__":
    main()
