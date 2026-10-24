#!/usr/bin/env python3
"""Tests for scripts/unicode_crosscheck.py: it fails on a difference, on a missing code point, on nothing compared,
and on a Python whose unicodedata is another Unicode version (it never skips; neither do these tests).

python3 scripts/test_unicode_crosscheck.py
"""
from __future__ import annotations

import importlib.util
import io
import tempfile
import unicodedata
import unittest
from contextlib import redirect_stdout
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("unicode_crosscheck", HERE / "unicode_crosscheck.py")
cc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cc)


def line(cp: int) -> str:
    x = chr(cp)
    forms = [cc.hexs(unicodedata.normalize(f, x)) for f in ("NFC", "NFD", "NFKC", "NFKD")] + [cc.hexs(x.casefold())]
    return f"{cp:X} " + " ".join(forms)


def run(lines: list[str]) -> tuple[int, str]:
    with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False, encoding="ascii") as f:
        f.write("\n".join(lines) + "\n")
    out = io.StringIO()
    with redirect_stdout(out):
        code = cc.main(["unicode_crosscheck.py", f.name])
    return code, out.getvalue()


class CrossCheck(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        # another version fails here, not skips (CI pins a Python whose unicodedata is the crate's version)
        assert unicodedata.unidata_version == cc.crate_version(), (
            f"Python's unicodedata is {unicodedata.unidata_version}, the crate's tables {cc.crate_version()}")
        cls.all = [line(cp) for cp in range(0x110000) if not 0xD800 <= cp <= 0xDFFF]

    def test_python_against_itself_passes(self):
        code, out = run(self.all)
        self.assertEqual(code, 0, out)
        self.assertIn("compared 1112064 code points", out)

    def test_a_difference_fails(self):
        planted = list(self.all)
        i = 0xC5  # Å: NFD is 41.30A
        f = planted[i].split()
        f[2] = "41"
        planted[i] = " ".join(f)
        code, out = run(planted)
        self.assertEqual(code, 1, out)
        self.assertIn("NFD(00C5)", out)

    def test_a_missing_code_point_fails(self):
        code, out = run(self.all[:-1])
        self.assertEqual(code, 1, out)
        self.assertIn("1 missing", out)

    def test_nothing_compared_fails(self):
        code, _ = run([])
        self.assertEqual(code, 1)


class Version(unittest.TestCase):
    def test_another_unicode_version_is_refused_not_skipped(self):
        real = cc.unicodedata
        class Other:
            unidata_version = "15.1.0"
        cc.unicodedata = Other
        try:
            out = io.StringIO()
            with redirect_stdout(out):
                code = cc.main(["unicode_crosscheck.py", "/dev/null"])
        finally:
            cc.unicodedata = real
        self.assertEqual(code, 1)
        self.assertIn("not compared", out.getvalue())


if __name__ == "__main__":
    unittest.main()
