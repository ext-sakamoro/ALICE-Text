#!/usr/bin/env python3
"""Compare the crate's normalization forms and case folding with Python's unicodedata, for every scalar value.

Reads the output of `cargo run --release --example unicode_dump` (stdin or a file) and checks each line against
unicodedata.normalize(NFC/NFD/NFKC/NFKD) and str.casefold() (the full default case folding). The Python build must
have the same Unicode version as the crate's tables (data/ucd/<version>/); another version is refused, not compared.

usage: cargo run --release --example unicode_dump | python3 scripts/unicode_crosscheck.py
exit 0: every line equal and 1 112 064 lines compared; 1: a difference, a missing line, or nothing compared
"""
from __future__ import annotations

import re
import sys
import unicodedata
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXPECTED_LINES = 0x110000 - 0x800  # every scalar value (the surrogates are not scalar values)


def crate_version() -> str:
    text = (ROOT / "src" / "unicode_tables.rs").read_text(encoding="utf-8")
    m = re.search(r"pub const UNICODE_VERSION: \(u8, u8, u8\) = \((\d+), (\d+), (\d+)\);", text)
    if m is None:
        sys.exit("no UNICODE_VERSION in src/unicode_tables.rs")
    return ".".join(m.groups())


def hexs(s: str) -> str:
    return ".".join(f"{ord(c):X}" for c in s)


def main(argv: list[str]) -> int:
    version = crate_version()
    if unicodedata.unidata_version != version:
        print(f"Python's unicodedata is {unicodedata.unidata_version}, the crate's tables are {version}: not compared")
        return 1
    stream = open(argv[1], encoding="ascii") if len(argv) > 1 else sys.stdin
    compared, diffs, seen = 0, [], set()
    for line in stream:
        f = line.split()
        if len(f) != 6:
            diffs.append(f"malformed line: {line.rstrip()}")
            continue
        cp = int(f[0], 16)
        seen.add(cp)
        x = chr(cp)
        want = [hexs(unicodedata.normalize(form, x)) for form in ("NFC", "NFD", "NFKC", "NFKD")] + [hexs(x.casefold())]
        for name, got, exp in zip(("NFC", "NFD", "NFKC", "NFKD", "fold"), f[1:], want):
            if got != exp and len(diffs) < 20:
                diffs.append(f"{name}({cp:04X}): crate {got}, Python {exp}")
        compared += 1
    missing = EXPECTED_LINES - len(seen)
    print(f"compared {compared} code points x 5 against unicodedata {unicodedata.unidata_version}: "
          f"{len(diffs)} differences, {missing} missing")
    for d in diffs:
        print(d)
    return 0 if compared == EXPECTED_LINES and not diffs and missing == 0 else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
