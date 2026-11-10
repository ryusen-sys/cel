#!/usr/bin/env python3
"""Update the shared workspace version without touching external dependencies."""

import pathlib
import re
import sys
import tomllib


def update(root: pathlib.Path, version: str) -> None:
    if not re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", version):
        raise ValueError("expected a stable semantic version")
    path = root / "Cargo.toml"
    text = path.read_text()
    workspace = tomllib.loads(text)["workspace"]
    old = workspace["package"]["version"]
    names = ["cel-core", "cel-core-proto", "cel-core-conformance"]
    text, count = re.subn(
        r'(\[workspace\.package\]\s*\nversion = ")[^"]+(")',
        lambda match: match[1] + version + match[2],
        text,
    )
    if count != 1:
        raise ValueError("workspace version field was not found exactly once")
    # Only local workspace dependencies carry the shared version.
    for name in names[:2]:
        pattern = rf'(?m)^({name} = \{{ path = "crates/{name}", version = ")[^"]+(" \}})$'
        text, count = re.subn(pattern, lambda match: match[1] + version + match[2], text)
        if count != 1:
            raise ValueError(f"local dependency {name} not found exactly once")
    lock_path = root / "Cargo.lock"
    lock = lock_path.read_text()
    for name in names:
        pattern = rf'(\[\[package\]\]\nname = "{name}"\nversion = "){re.escape(old)}("\n)'
        lock, count = re.subn(pattern, lambda match: match[1] + version + match[2], lock)
        if count != 1:
            raise ValueError(f"workspace lock entry {name} not found exactly once")
    tomllib.loads(text)
    tomllib.loads(lock)
    path.write_text(text)
    lock_path.write_text(lock)


if __name__ == "__main__":
    update(pathlib.Path.cwd(), sys.argv[1])
