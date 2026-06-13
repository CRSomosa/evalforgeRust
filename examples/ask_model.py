#!/usr/bin/env python3
"""Bridge script: lets evalforge's `cmd` provider talk to any HTTP API.

evalforge pipes the prompt to this script's stdin and reads the
completion from stdout:

    cargo run -- run tasks/sample.eval --provider cmd --cmd "python examples/ask_model.py"

Fill in the API of your choice below. Example shown for an
OpenAI-compatible endpoint; adapt freely.
"""
import json
import os
import sys
import urllib.request

prompt = sys.stdin.read()

req = urllib.request.Request(
    "https://api.openai.com/v1/chat/completions",
    headers={
        "Content-Type": "application/json",
        "Authorization": f"Bearer {os.environ['OPENAI_API_KEY']}",
    },
    data=json.dumps({
        "model": "gpt-4o-mini",
        "messages": [{"role": "user", "content": prompt}],
    }).encode(),
)

with urllib.request.urlopen(req) as resp:
    body = json.load(resp)

print(body["choices"][0]["message"]["content"])
