#!/usr/bin/env python3
"""Ask a model about a routine with a reference document in front of it.

    python3 tools/ask-with-reference.py prompts/z80-routine-analysis.md \
        /tmp/manic.jsonl --addresses 8A75,8DAA,8ABB,925F,92CB

The document goes in as the system prompt, the routine's flow listing and
measurements as the question. The context length is set explicitly: Ollama
defaults to a few thousand tokens, and a reference of this size would be
quietly truncated, which would make the comparison meaningless rather than
merely disappointing.
"""

import argparse
import json
import sys
import urllib.request


def question(episode):
    parts = [f"Routine at ${episode['address']} of a ZX Spectrum game."]
    w, i = episode["writes"], episode["inclusive"]
    span = episode.get("wrote_between") or ["-", "-"]
    parts.append(
        "Measured while the game ran: called {calls} times in {frames} frames; "
        "wrote {ws} bytes to $4000-$57FF, {wa} to $5800-$5AFF and {wo} elsewhere "
        "by its own instructions, and {is_} / {ia} / {io} counting the routines it "
        "calls; every address it wrote lay between ${lo} and ${hi}.".format(
            calls=episode["calls"], frames=episode["frames_seen"],
            ws=w["screen"], wa=w["attrs"], wo=w["other"],
            is_=i["screen"], ia=i["attrs"], io=i["other"],
            lo=span[0], hi=span[1],
        )
    )
    if episode.get("callers"):
        parts.append("Called from: " + ", ".join("$" + c for c in episode["callers"]))
    if episode.get("examples"):
        parts.append("Registers it was handed:\n  " + "\n  ".join(episode["examples"]))
    flow = episode.get("flow") or episode.get("listing") or []
    parts.append("Its code, with loops marked and their measured trip counts:\n"
                 + "\n".join(flow))
    return "\n\n".join(parts)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("reference")
    parser.add_argument("episodes")
    parser.add_argument("--addresses", default="")
    parser.add_argument("--model", default="qwen2.5-coder:7b")
    parser.add_argument("--context", type=int, default=16384)
    args = parser.parse_args()

    reference = open(args.reference, encoding="utf-8").read()
    episodes = {e["address"]: e for e in (json.loads(l) for l in open(args.episodes))}
    wanted = [a.strip().upper() for a in args.addresses.split(",") if a.strip()]

    for address in wanted or list(episodes)[:5]:
        episode = episodes.get(address)
        if not episode:
            print(f"${address}: not in the trace", file=sys.stderr)
            continue
        body = json.dumps({
            "model": args.model,
            "system": reference,
            "prompt": question(episode),
            "stream": False,
            "keep_alive": "10m",
            "options": {"temperature": 0, "num_predict": 320, "num_ctx": args.context},
        }).encode()
        request = urllib.request.Request(
            "http://127.0.0.1:11434/api/generate", data=body,
            headers={"Content-Type": "application/json"},
        )
        with urllib.request.urlopen(request, timeout=900) as reply:
            said = json.load(reply)
        print(f"===== ${address} " + "=" * 50)
        print(said.get("response", "").strip())
        print()


if __name__ == "__main__":
    main()
