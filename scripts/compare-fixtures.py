#!/usr/bin/env python3
"""Compares two fixture trees, ignoring what legitimately differs between captures:
timestamps, uuids, dates in headers and the Created/Updated lines in CLI output.

    scripts/compare-fixtures.py tests/fixtures /tmp/recaptured
"""
import json
import pathlib
import sys

VOLATILE_KEYS = {"createdAt", "updatedAt", "uuid"}
KEPT_HEADERS = {"content-type", "www-authenticate", "allow", "location"}


def stable(value):
    if isinstance(value, dict):
        return {k: stable(v) for k, v in value.items() if k not in VOLATILE_KEYS}
    if isinstance(value, list):
        return [stable(v) for v in value]
    return value


def normalise(path: pathlib.Path):
    text = path.read_text(errors="replace")
    name = path.name
    if name.endswith(".headers") or text.startswith("HTTP/"):
        # A header dump, including a body captured with `curl -I`.
        lines = [l.strip() for l in text.splitlines()]
        status = lines[0] if lines else ""
        kept = sorted(l for l in lines[1:] if l.split(":", 1)[0].lower() in KEPT_HEADERS)
        return [status, *kept]
    if name.endswith(".txt"):
        return [l for l in text.splitlines() if not l.startswith(("Created At:", "Updated At:"))]
    try:
        return stable(json.loads(text))
    except json.JSONDecodeError:
        return text


def main(a: str, b: str) -> int:
    left, right = pathlib.Path(a), pathlib.Path(b)
    names = {p.relative_to(left) for p in left.rglob("*") if p.is_file()}
    names |= {p.relative_to(right) for p in right.rglob("*") if p.is_file()}
    problems = 0
    for rel in sorted(names):
        l, r = left / rel, right / rel
        if not l.exists() or not r.exists():
            print(f"only in {'left' if l.exists() else 'right'}: {rel}")
            problems += 1
        elif normalise(l) != normalise(r):
            print(f"differs: {rel}")
            problems += 1
    print(f"{len(names)} files compared, {problems} differences")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1], sys.argv[2]))
