#!/usr/bin/env python3
"""Check fenced ace examples against the built CLI without opening design files."""
import argparse
import json
from pathlib import Path
import shlex
import subprocess
import sys


def commands(text):
    shell = False
    for line in text.splitlines():
        if line.startswith("```"):
            shell = not shell and line[3:].strip() in {"bash", "sh", "powershell", "console"}
            continue
        if not shell or not line.strip().startswith("ace "):
            continue
        lexer = shlex.shlex(line, posix=True, punctuation_chars=";&|><")
        lexer.whitespace_split = True
        tokens = list(lexer)
        for i, token in enumerate(tokens):
            if token in {";", "&&", "|", ">", ">>", "2>"}:
                tokens = tokens[:i]
                break
        yield tokens


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ace", default="target/debug/ace")
    args = parser.parse_args()
    binary = str(Path(args.ace).resolve())
    root = Path(__file__).resolve().parent.parent
    examples = []
    for path in [root / "README.md", root / "skills/altium-cli-extended/SKILL.md"]:
        examples.extend((str(path.relative_to(root)), command) for command in commands(path.read_text()))
    manifest = subprocess.run([binary, "commands", "--json"], capture_output=True, check=True, text=True)
    for row in json.loads(manifest.stdout)["data"]["items"]:
        examples.append(("META: " + row["path"], shlex.split(row["example"])))
    failures = []
    for source, command in examples:
        result = subprocess.run([binary, *command[1:], "--help"], capture_output=True, text=True)
        if result.returncode:
            failures.append(f"{source}: {shlex.join(command)}\n{result.stdout}{result.stderr}")
    print(f"Checked {len(examples)} ace examples; {len(failures)} broken examples.")
    for failure in failures:
        print(failure, file=sys.stderr)
    return int(bool(failures))


if __name__ == "__main__":
    sys.exit(main())
