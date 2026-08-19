#!/usr/bin/env python3
"""Score a model's answers against the names in a real disassembly.

    python3 tools/score-model.py truth.json said-full.jsonl said-measured.jsonl ...

`truth.json` maps an address to the name somebody who understood the game gave
the routine — DRAWSHEET, MOVEWILLY, DECAIR. Those names are short and blunt,
which makes them better ground truth than the prose around them: there is
little room to argue about what DRAWHG is for.

An answer counts as right when it lands in the same category as the name. The
categories are the ones the rules are scored against, so the numbers can be put
side by side.
"""

import json
import re
import sys

# What the names mean, in the same terms the rest of the scoring uses. Kept
# separate from the description keywords because a name is not a sentence:
# "DRAWHG" has no spaces to match words against.
NAME_MEANS = [
    (r"draw|plot|print|sprite|paint", "draw"),
    (r"move|walk|jump|fall|anim", "move"),
    (r"attr|colour|color|ink|paper|flash", "colour"),
    (r"key|input|control", "input"),
    (r"sound|beep|note|tune|music", "sound"),
    (r"score|bonus|hiscore", "score"),
    (r"air|life|lives|death|die|kill", "state"),
    (r"init|reset|start|setup", "setup"),
    (r"copy|clear|fill|blit", "copy"),
    (r"loop|main|game", "logic"),
]

# What an answer means, from the words a model actually uses.
SAID_MEANS = [
    (r"\bdraw|\bplot|\bprint|sprite|render|display file|screen", "draw"),
    (r"\bmove|walk|jump|fall|animat|position|direction", "move"),
    (r"attribute|colour|color", "colour"),
    (r"keyboard|\bkey\b|input|joystick|control", "input"),
    (r"sound|beep|note|tune|music|speaker", "sound"),
    (r"score|bonus|points", "score"),
    (r"\bair\b|\blife|lives|death|dies|dying", "state"),
    (r"initialis|initializ|\breset|set up|setup", "setup"),
    (r"\bcop(y|ies)|clear|fill|block of bytes|transfer", "copy"),
    (r"main loop|game loop|counter|calls another", "logic"),
]


def categories(text, table):
    text = text.lower()
    return {name for pattern, name in table if re.search(pattern, text)}


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    truth = json.load(open(sys.argv[1], encoding="utf-8"))

    for path in sys.argv[2:]:
        answers = [json.loads(line) for line in open(path, encoding="utf-8")]
        right = wrong = declined = unscorable = 0
        misses = []

        for answer in answers:
            said = answer["said"].strip()
            name = truth.get(answer["address"], "")
            meant = categories(name, NAME_MEANS)
            if not said:
                declined += 1
                continue
            if said.lower().startswith("unknown"):
                declined += 1
                continue
            if not meant:
                # The name says nothing this can check.
                unscorable += 1
                continue
            if categories(said, SAID_MEANS) & meant:
                right += 1
            else:
                wrong += 1
                misses.append((answer["address"], name, said[:60]))

        scored = right + wrong
        print(f"\n{path}")
        print(f"  {len(answers)} routines, {declined} declined, {unscorable} name says nothing")
        print(f"  scored   {scored}")
        print(f"  RIGHT    {right} ({right * 100 // scored if scored else 0}%)")
        for address, name, said in misses[:6]:
            print(f"    ${address} is {name}, said: {said}")


if __name__ == "__main__":
    main()
