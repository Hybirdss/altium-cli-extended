#!/usr/bin/env python3
"""Refuse a release tag that disagrees with the binary crate version."""
import json
import subprocess
import sys

metadata = json.loads(subprocess.check_output(
    ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"], text=True))
version = next(p["version"] for p in metadata["packages"] if p["name"] == "altium-cli-extended")
expected = f"v{version}"
if len(sys.argv) != 2 or sys.argv[1] != expected:
    sys.exit(f"release tag must equal {expected}; received {sys.argv[1:]}")
print(f"release tag {expected} matches altium-cli-extended {version}")
