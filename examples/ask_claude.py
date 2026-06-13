#!/usr/bin/env python3
"""Bridge script: pipes a prompt to the Claude API and prints the reply.

evalforge sends each task's prompt to this script's stdin and reads
the completion from stdout:

    cargo run -- run tasks/sample.eval --provider cmd --cmd "python examples/ask_claude.py"

Requires a .env file in the project root (next to Cargo.toml):
    ANTHROPIC_API_KEY=sk-ant-your-key-here

Change MODEL below to try different Claude versions:
    claude-haiku-4-5-20251001   (fastest, cheapest)
    claude-sonnet-4-6            (smarter, moderate cost)
    claude-opus-4-8              (most capable)
"""
import json
import os
import sys
import urllib.request
from pathlib import Path

MODEL = "claude-haiku-4-5-20251001"

# --- load .env from the project root (two levels up from examples/) ---
env_path = Path(__file__).parent.parent / ".env"
if env_path.exists():
    for line in env_path.read_text().splitlines():
        line = line.strip()
        if line and not line.startswith("#") and "=" in line:
            key, _, value = line.partition("=")
            os.environ.setdefault(key.strip(), value.strip())

api_key = os.environ.get("ANTHROPIC_API_KEY", "")
if not api_key or api_key == "sk-ant-your-key-here":
    print("[ask_claude.py] error: set ANTHROPIC_API_KEY in .env or the environment", file=sys.stderr)
    sys.exit(1)

prompt = sys.stdin.read().strip()

payload = json.dumps({
    "model": MODEL,
    "max_tokens": 1024,
    "messages": [{"role": "user", "content": prompt}],
}).encode()

req = urllib.request.Request(
    "https://api.anthropic.com/v1/messages",
    headers={
        "Content-Type": "application/json",
        "x-api-key": api_key,
        "anthropic-version": "2023-06-01",
    },
    data=payload,
)

try:
    with urllib.request.urlopen(req) as resp:
        body = json.load(resp)
    print(body["content"][0]["text"])
except urllib.error.HTTPError as e:
    error_body = e.read().decode()
    print(f"[ask_claude.py] HTTP {e.code}: {error_body}", file=sys.stderr)
    sys.exit(1)
