#!/usr/bin/env python3
"""The checked-in Unicode data cannot be changed quietly, not even with its SHA256SUMS edited to match.

The vocabulary lint trusts a file under data/ucd/ while its hash is in the SHA256SUMS next to it, so SHA256SUMS must
not be the weak point. Each SHA256SUMS is hashed into a semantics id that the Rust tests pin as a literal, and into
the header of the generated src/unicode_tables.rs that the generator's --check compares. This test shows both:
the files match their lists, the lists give the pinned ids, and a data file edited together with its list gives
another id and another generated file.

python3 scripts/test_unicode_data_pin.py
"""
from __future__ import annotations

import hashlib
import importlib.util
import re
import shutil
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("gen_unicode_tables", ROOT / "scripts" / "gen_unicode_tables.py")
gen = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gen)

# (data directory, the id text's first line, the Rust test that pins the id)
PINS = [
    (gen.DATA, "alice-text unicode v1", ROOT / "tests" / "unicode_conformance.rs"),
    (gen.WB_DATA, "alice-text word break v1", ROOT / "tests" / "word_break.rs"),
]


def pinned_id(test_file: Path) -> str:
    m = re.findall(r'const ID_HEX: &str = "([0-9a-f]{64})";', test_file.read_text(encoding="utf-8"))
    assert len(m) == 1, (test_file, m)
    return m[0]


def id_of(directory: Path, head: str) -> str:
    sums = (directory / "SHA256SUMS").read_text(encoding="utf-8")
    return hashlib.sha256(f"{head}\nunicode 16.0.0\n{sums}".encode()).hexdigest()


def rewrite_sums(directory: Path) -> None:
    lines = []
    for line in (directory / "SHA256SUMS").read_text(encoding="utf-8").splitlines():
        _, name = line.split(maxsplit=1)
        lines.append(f"{hashlib.sha256((directory / name).read_bytes()).hexdigest()}  {name}")
    (directory / "SHA256SUMS").write_text("\n".join(lines) + "\n", encoding="utf-8")


class Pins(unittest.TestCase):
    def test_every_file_matches_its_list_and_every_list_gives_the_pinned_id(self):
        compared = 0
        for directory, head, test_file in PINS:
            for line in (directory / "SHA256SUMS").read_text(encoding="utf-8").splitlines():
                digest, name = line.split(maxsplit=1)
                self.assertEqual(hashlib.sha256((directory / name).read_bytes()).hexdigest(), digest, name)
                compared += 1
            self.assertEqual(id_of(directory, head), pinned_id(test_file), directory)
        self.assertEqual(compared, 7)

    def test_a_data_file_edited_with_its_list_gives_another_id_and_another_generated_file(self):
        current = (ROOT / "src" / "unicode_tables.rs").read_text(encoding="utf-8")
        for directory, head, test_file in PINS:
            with tempfile.TemporaryDirectory() as d:
                copy = Path(d) / "16.0.0"
                shutil.copytree(gen.DATA, copy)
                target = copy if directory == gen.DATA else copy / "word-break"
                name = (target / "SHA256SUMS").read_text(encoding="utf-8").split()[1]
                with open(target / name, "a", encoding="utf-8") as f:
                    f.write("# an added comment line\n")
                rewrite_sums(target)
                # the list now matches the edited file, and the id moves off the literal the Rust test pins
                self.assertNotEqual(id_of(target, head), pinned_id(test_file), directory)
                # and the generator, reading the edited copy, writes another file than the checked-in one
                saved = (gen.DATA, gen.WB_DATA)
                gen.DATA, gen.WB_DATA = copy, copy / "word-break"
                try:
                    self.assertNotEqual(gen.generate(), current, directory)
                finally:
                    gen.DATA, gen.WB_DATA = saved


if __name__ == "__main__":
    unittest.main()
