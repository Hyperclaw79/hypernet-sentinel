"""Offline regression tests for deterministic upstream snapshot generation."""

from __future__ import annotations

import hashlib
import tempfile
import unittest
from pathlib import Path

import update_upstream


class LineEndingTests(unittest.TestCase):
    def test_write_lf_text_normalizes_all_line_endings(self) -> None:
        with tempfile.TemporaryDirectory() as temp_name:
            output = Path(temp_name) / "generated.rs"
            update_upstream.write_lf_text(output, "one\r\ntwo\rthree\nfour\r\n")

            self.assertEqual(output.read_bytes(), b"one\ntwo\nthree\nfour\n")

    def test_manifest_hashes_match_normalized_on_disk_bytes(self) -> None:
        with tempfile.TemporaryDirectory() as temp_name:
            staged = Path(temp_name)
            (staged / "engine").mkdir()
            source = staged / "engine" / "sample.rs"
            update_upstream.write_lf_text(source, "pub fn sample() {}\r\n")

            hashes = update_upstream.source_hashes(staged)

            expected = hashlib.sha256(b"pub fn sample() {}\n").hexdigest()
            self.assertEqual(hashes, {"engine/sample.rs": expected})
            self.assertNotIn(b"\r", source.read_bytes())


if __name__ == "__main__":
    unittest.main()
