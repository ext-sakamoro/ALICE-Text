#!/usr/bin/env python3
"""Generate src/unicode_tables.rs from the Unicode Character Database at the pinned version.

Reads data/ucd/<version>/{UnicodeData,CompositionExclusions,CaseFolding}.txt, refuses to run unless every file
has the SHA-256 recorded in data/ucd/<version>/SHA256SUMS, and writes the tables the normalization forms and the
case folding read. Nothing comes from the running Python's own Unicode tables or the platform's.

usage: python3 scripts/gen_unicode_tables.py            write src/unicode_tables.rs
       python3 scripts/gen_unicode_tables.py --check    exit 1 if src/unicode_tables.rs differs from what it writes
"""
from __future__ import annotations

import hashlib
import sys
from pathlib import Path

VERSION = (16, 0, 0)
ROOT = Path(__file__).resolve().parent.parent
DATA = ROOT / "data" / "ucd" / ".".join(map(str, VERSION))
OUT = ROOT / "src" / "unicode_tables.rs"
FILES = ["CaseFolding.txt", "CompositionExclusions.txt", "NormalizationTest.txt", "UnicodeData.txt"]
WB_DATA = DATA / "word-break"
WB_FILES = ["WordBreakProperty.txt", "WordBreakTest.txt", "emoji-data.txt"]
# the Word_Break values, in the order of the generated `Wb` enum (Other is 0: every code point not listed)
WB_VALUES = ["Other", "CR", "LF", "Newline", "Extend", "ZWJ", "Regional_Indicator", "Format", "Katakana",
             "Hebrew_Letter", "ALetter", "Single_Quote", "Double_Quote", "MidNumLet", "MidLetter", "MidNum",
             "Numeric", "ExtendNumLet", "WSegSpace"]

S_BASE, L_BASE, V_BASE, T_BASE = 0xAC00, 0x1100, 0x1161, 0x11A7
L_COUNT, V_COUNT, T_COUNT = 19, 21, 28
S_COUNT = L_COUNT * V_COUNT * T_COUNT


def checked_sums(directory: Path | None = None, files: list[str] | None = None) -> str:
    directory, files = directory or DATA, files or FILES
    sums = (directory / "SHA256SUMS").read_text(encoding="utf-8")
    listed = {}
    for line in sums.splitlines():
        digest, name = line.split(maxsplit=1)
        listed[name.strip()] = digest
    if sorted(listed, key=str.lower) != sorted(files, key=str.lower):
        sys.exit(f"{directory}/SHA256SUMS lists {sorted(listed)}, not {files}")
    for name, digest in listed.items():
        got = hashlib.sha256((directory / name).read_bytes()).hexdigest()
        if got != digest:
            sys.exit(f"{name}: sha256 {got}, SHA256SUMS says {digest}")
    return sums


def wb_name(value: str) -> str:
    """the Rust variant of a Word_Break value (CR / LF / ZWJ as Cr / Lf / Zwj)"""
    return {"CR": "Cr", "LF": "Lf", "ZWJ": "Zwj"}.get(value, value.replace("_", ""))


def property_ranges(path: Path, wanted: dict[str, int]) -> dict[int, int]:
    """{code point: value} from a `range ; Property` file, for the property names in `wanted`"""
    out = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.split("#")[0].strip()
        if not line:
            continue
        cps, prop = [x.strip() for x in line.split(";")[:2]]
        if prop not in wanted:
            continue
        a, _, b = cps.partition("..")
        for cp in range(int(a, 16), int(b or a, 16) + 1):
            if cp in out and out[cp] != wanted[prop]:
                sys.exit(f"{path.name}: {cp:04X} has two values")
            out[cp] = wanted[prop]
    return out


def unicode_data():
    ccc, canon, compat = {}, {}, {}
    for line in (DATA / "UnicodeData.txt").read_text(encoding="utf-8").splitlines():
        f = line.split(";")
        cp = int(f[0], 16)
        if int(f[3]):
            ccc[cp] = int(f[3])
        if f[5]:
            parts = f[5].split()
            if parts[0].startswith("<"):
                compat[cp] = [int(x, 16) for x in parts[1:]]
            else:
                canon[cp] = [int(x, 16) for x in parts]
    return ccc, canon, compat


def exclusions() -> set[int]:
    out = set()
    for line in (DATA / "CompositionExclusions.txt").read_text(encoding="utf-8").splitlines():
        line = line.split("#")[0].strip()
        if not line:
            continue
        if ".." in line:
            a, b = line.split("..")
            out.update(range(int(a, 16), int(b, 16) + 1))
        else:
            out.add(int(line, 16))
    return out


def case_folding():
    full, simple = {}, {}
    for line in (DATA / "CaseFolding.txt").read_text(encoding="utf-8").splitlines():
        line = line.split("#")[0].strip()
        if not line:
            continue
        code, status, mapping = [x.strip() for x in line.split(";")[:3]]
        cp, m = int(code, 16), [int(x, 16) for x in mapping.split()]
        if status in ("C", "F"):
            full[cp] = m
        if status in ("C", "S"):
            simple[cp] = m[0]
    return full, simple


def full_decomposition(cp: int, canon: dict, compat: dict | None) -> list[int]:
    m = (compat or {}).get(cp) or canon.get(cp)
    if m is None:
        return [cp]
    out = []
    for c in m:
        out.extend(full_decomposition(c, canon, compat))
    return out


def ranges(values: dict[int, int]) -> list[tuple[int, int, int]]:
    out: list[list[int]] = []
    for cp in sorted(values):
        v = values[cp]
        if out and out[-1][1] == cp - 1 and out[-1][2] == v:
            out[-1][1] = cp
        else:
            out.append([cp, cp, v])
    return [tuple(r) for r in out]


def flat(mapping: dict[int, list[int]], name: str) -> list[str]:
    index, data = [], []
    for cp in sorted(mapping):
        seq = mapping[cp]
        index.append(f"({cp:#07x}, {len(data)}, {len(seq)})")
        data.extend(seq)
    lines = [f"pub static {name}: &[(u32, u16, u8)] = &["]
    lines += [f"    {', '.join(index[i:i + 6])}," for i in range(0, len(index), 6)]
    lines += ["];", f"pub static {name}_DATA: &[u32] = &["]
    lines += [f"    {', '.join(f'{x:#07x}' for x in data[i:i + 10])}," for i in range(0, len(data), 10)]
    lines += ["];"]
    assert len(data) < 65536 and all(len(s) < 256 for s in mapping.values())
    return lines


def generate() -> str:
    sums = checked_sums()
    ccc, canon, compat = unicode_data()
    excl = exclusions()
    fold_full, fold_simple = case_folding()

    canon_full = {cp: full_decomposition(cp, canon, None) for cp in canon}
    compat_full = {}
    for cp in set(canon) | set(compat):
        d = full_decomposition(cp, canon, compat)
        if d != canon_full.get(cp, [cp]):
            compat_full[cp] = d
    compose = []
    for cp, m in canon.items():
        if len(m) != 2 or cp in excl or ccc.get(cp, 0) != 0 or ccc.get(m[0], 0) != 0:
            continue
        compose.append((m[0], m[1], cp))
    compose.sort()

    version = ".".join(map(str, VERSION))
    sid = hashlib.sha256(f"alice-text unicode v1\nunicode {version}\n{sums}".encode()).digest()
    out = [
        f"// @generated by scripts/gen_unicode_tables.py from data/ucd/{version}/ (UCD {version}); do not edit.",
        "// The input files and their SHA-256 are listed in data/ucd/" + version + "/SHA256SUMS:",
    ]
    out += [f"//   {line}" for line in sums.splitlines()]
    out += [
        "",
        f"/// The Unicode version the tables are generated from.",
        f"pub const UNICODE_VERSION: (u8, u8, u8) = ({VERSION[0]}, {VERSION[1]}, {VERSION[2]});",
        "",
        "/// SHA-256 of `alice-text unicode v1\\nunicode <version>\\n` and the input files' SHA256SUMS: another",
        "/// Unicode version, or other data, is another id.",
        "pub const NORMALIZATION_SEMANTICS_ID: [u8; 32] = [",
        "    " + ", ".join(f"{b:#04x}" for b in sid[:16]) + ",",
        "    " + ", ".join(f"{b:#04x}" for b in sid[16:]) + ",",
        "];",
        "",
        "/// Canonical combining class, as (first, last, class) ranges sorted by code point (class 0 elsewhere).",
        "pub const CCC: &[(u32, u32, u8)] = &[",
    ]
    rr = ranges(ccc)
    out += [f"    {', '.join(f'({a:#07x}, {b:#07x}, {c})' for a, b, c in rr[i:i + 4])}," for i in range(0, len(rr), 4)]
    out += ["];", "", "/// Full canonical decomposition (index into `CANON_DATA`: code point, offset, length)."]
    out += flat(canon_full, "CANON")
    out += ["", "/// Full compatibility decomposition, where it differs from the canonical one."]
    out += flat(compat_full, "COMPAT")
    out += ["", "/// Primary composites: (first, second, composite), sorted by (first, second).",
            "pub static COMPOSE: &[(u32, u32, u32)] = &["]
    out += [f"    {', '.join(f'({a:#07x}, {b:#07x}, {c:#07x})' for a, b, c in compose[i:i + 4])},"
            for i in range(0, len(compose), 4)]
    out += ["];", "", "/// Full case folding (status C and F)."]
    out += flat(fold_full, "FOLD_FULL")
    out += ["", "/// Simple case folding (status C and S): (code point, folding).",
            "pub static FOLD_SIMPLE: &[(u32, u32)] = &["]
    fs = sorted(fold_simple.items())
    out += [f"    {', '.join(f'({a:#07x}, {b:#07x})' for a, b in fs[i:i + 6])}," for i in range(0, len(fs), 6)]
    out += ["];", ""]

    # UAX #29 word boundaries: the Word_Break property and Extended_Pictographic (WB3c)
    wb_sums = checked_sums(WB_DATA, WB_FILES)
    wb = property_ranges(WB_DATA / "WordBreakProperty.txt", {v: i for i, v in enumerate(WB_VALUES) if i})
    pict = property_ranges(WB_DATA / "emoji-data.txt", {"Extended_Pictographic": 1})
    wb_id = hashlib.sha256(f"alice-text word break v1\nunicode {version}\n{wb_sums}".encode()).digest()
    out += [
        "// UAX #29 word boundaries, from data/ucd/" + version + "/word-break/ (SHA256SUMS there):",
    ]
    out += [f"//   {line}" for line in wb_sums.splitlines()]
    out += [
        "",
        "/// SHA-256 of `alice-text word break v1\\nunicode <version>\\n` and the word-break files' SHA256SUMS.",
        "pub const WORD_BREAK_SEMANTICS_ID: [u8; 32] = [",
        "    " + ", ".join(f"{b:#04x}" for b in wb_id[:16]) + ",",
        "    " + ", ".join(f"{b:#04x}" for b in wb_id[16:]) + ",",
        "];",
        "",
        "/// The `Word_Break` values (`Other` for every code point the property file does not list).",
        "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
        "pub enum Wb {",
    ]
    out += [f"    {wb_name(v)}," for v in WB_VALUES]
    out += [
        "}",
        "",
        "/// `Word_Break`, as (first, last, value) ranges sorted by code point.",
        "pub const WORD_BREAK: &[(u32, u32, Wb)] = &[",
    ]
    names = [wb_name(v) for v in WB_VALUES]
    wr = ranges(wb)
    out += [f"    {', '.join(f'({a:#07x}, {b:#07x}, Wb::{names[c]})' for a, b, c in wr[i:i + 3])},"
            for i in range(0, len(wr), 3)]
    out += ["];", "", "/// `Extended_Pictographic`, as (first, last) ranges sorted by code point.",
            "pub const EXTENDED_PICTOGRAPHIC: &[(u32, u32)] = &["]
    pr = [(a, b) for a, b, _ in ranges(pict)]
    out += [f"    {', '.join(f'({a:#07x}, {b:#07x})' for a, b in pr[i:i + 6])}," for i in range(0, len(pr), 6)]
    out += ["];", ""]
    assert S_COUNT == 11172
    return "\n".join(out)


def main(argv: list[str]) -> int:
    text = generate()
    if argv[1:] == ["--check"]:
        if not OUT.exists() or OUT.read_text(encoding="utf-8") != text:
            print(f"{OUT.relative_to(ROOT)} differs from what the generator writes: run scripts/gen_unicode_tables.py")
            return 1
        print(f"{OUT.relative_to(ROOT)}: as generated (UCD {'.'.join(map(str, VERSION))})")
        return 0
    if argv[1:]:
        print(__doc__)
        return 2
    OUT.write_text(text, encoding="utf-8")
    print(f"wrote {OUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
