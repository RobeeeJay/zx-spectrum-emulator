#!/usr/bin/env python3
"""Ask a model for a claim it has to back with a real number, and check it.

A model that invents a name will invent the evidence for it too: asked to
quote the measurement supporting its answer, the 7B model cited "0 bytes
written" in support of a routine that writes attributes, and a figure for an
address range that was never given to it. So the citation is checked here
rather than believed.

    python3 tools/checked-ask.py manic.jsonl --model qwen2.5-coder:7b

Every answer must name a field of the episode and the value it holds. If the
field does not exist, or holds something else, the answer is thrown away — the
model is told what was wrong and asked once more, and if it cannot cite
something real the routine is left unnamed. An unnamed routine costs a reader
nothing; a confidently wrong one costs them the time to disprove it.
"""

import argparse, json, sys, urllib.request

VOCABULARY = [
    "draws", "moves something", "reads input", "makes a noise", "copies a block",
    "keeps a counter", "decompresses", "checks a condition", "no evidence",
]

SYSTEM = f"""You are reading measurements of a routine from a ZX Spectrum game.

Answer with one line of JSON and nothing else:
{{"does": "<one of: {', '.join(VOCABULARY)}>", "name": "<a short lower_case name>", "field": "<the field you are relying on>", "value": <its value>}}

"field" must be one of: calls, writes.screen, writes.attrs, writes.other,
inclusive.screen, inclusive.attrs, inclusive.other, longest_loop, frames_seen.
"value" must be exactly what that field holds in what you were given. It will
be checked. If nothing in the measurements supports a guess, answer
{{"does": "no evidence"}} — that is a useful answer, an invented one is not."""


def field_of(episode, path):
    """The value of a dotted field, or None if there is no such field."""
    at = episode
    for part in path.split("."):
        if not isinstance(at, dict) or part not in at:
            return None
        at = at[part]
    return at


def ask(model, prompt, timeout=180):
    body = json.dumps({"model": model, "system": SYSTEM, "prompt": prompt, "stream": False,
                       "keep_alive": "10m",
                       "options": {"temperature": 0, "num_predict": 120}}).encode()
    request = urllib.request.Request("http://127.0.0.1:11434/api/generate", data=body,
                                     headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=timeout) as reply:
        return json.load(reply).get("response", "").strip()


def described(episode):
    w, i = episode["writes"], episode["inclusive"]
    return (f"Routine ${episode['address']}. calls={episode['calls']}, "
            f"frames_seen={episode['frames_seen']}, longest_loop={episode['longest_loop']}, "
            f"writes.screen={w['screen']}, writes.attrs={w['attrs']}, writes.other={w['other']}, "
            f"inclusive.screen={i['screen']}, inclusive.attrs={i['attrs']}, "
            f"inclusive.other={i['other']}.")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("episodes")
    parser.add_argument("--model", default="qwen2.5-coder:7b")
    parser.add_argument("--limit", type=int, default=0)
    args = parser.parse_args()

    episodes = [json.loads(l) for l in open(args.episodes, encoding="utf-8")]
    if args.limit:
        episodes = episodes[: args.limit]

    kept = invented = declined = unparsed = 0
    for episode in episodes:
        prompt = described(episode)
        verdict = "unparsed"
        for attempt in range(2):
            said = ask(args.model, prompt)
            start, end = said.find("{"), said.rfind("}")
            if start < 0 or end < 0:
                continue
            try:
                answer = json.loads(said[start : end + 1])
            except json.JSONDecodeError:
                continue

            if answer.get("does") == "no evidence":
                verdict = "declined"
                break
            field, claimed = answer.get("field"), answer.get("value")
            actual = field_of(episode, field or "")
            if actual is None:
                prompt = f"{described(episode)}\n\nThere is no field called {field!r}. Use one of the fields listed."
                verdict = "invented"
                continue
            if actual != claimed:
                prompt = (f"{described(episode)}\n\nYou said {field} was {claimed}; it is {actual}. "
                          f"Quote the value as given.")
                verdict = "invented"
                continue
            verdict = "kept"
            print(f"${episode['address']}  {answer.get('name','?'):24} {answer.get('does','?'):18} "
                  f"because {field}={actual}")
            break

        if verdict == "kept": kept += 1
        elif verdict == "declined": declined += 1
        elif verdict == "invented": invented += 1
        else: unparsed += 1

    total = len(episodes)
    print(f"\n{total} routines: {kept} claims backed by a real measurement, "
          f"{declined} declined, {invented} cited something that was not there, "
          f"{unparsed} unreadable", file=sys.stderr)


if __name__ == "__main__":
    main()
