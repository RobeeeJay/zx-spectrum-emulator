#!/usr/bin/env python3
"""Send a file of text to a model and print what comes back.

    python3 tools/send-prompt.py prompt.txt
    python3 tools/send-prompt.py prompt.txt --model qwen2.5-coder:14b
    python3 tools/send-prompt.py prompt.txt --model claude

Nothing is added to what is in the file: edit it and run this again. Through
the HTTP API rather than `ollama run`, which draws a progress spinner into its
own output and lets the model fall out of memory between questions.
"""

import argparse
import json
import subprocess
import sys
import urllib.request


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("prompt")
    parser.add_argument("--model", default="qwen2.5-coder:7b")
    parser.add_argument("--tokens", type=int, default=500)
    parser.add_argument("--temperature", type=float, default=0.0)
    args = parser.parse_args()

    text = open(args.prompt, encoding="utf-8").read()

    if args.model == "claude":
        done = subprocess.run(["claude", "-p"], input=text, capture_output=True, text=True)
        print(done.stdout.strip())
        return

    body = json.dumps({
        "model": args.model,
        "prompt": text,
        "stream": False,
        "keep_alive": "10m",
        "options": {"temperature": args.temperature, "num_predict": args.tokens},
    }).encode()
    request = urllib.request.Request(
        "http://127.0.0.1:11434/api/generate",
        data=body,
        headers={"Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(request, timeout=900) as reply:
            print(json.load(reply)["response"].strip())
    except urllib.error.URLError as e:
        sys.exit(f"could not reach ollama: {e}. Is `ollama serve` running?")


if __name__ == "__main__":
    main()
