#!/usr/bin/env python3
"""Exercise report artifact encoding, replacement and failure cleanup without cmux."""
import base64
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import test_terminal_input_render_report as report


class ReportOutput(unittest.TestCase):
    """Verify the shared legacy report writer independently of debug snapshot APIs."""

    def test_embeds_image_and_escapes_metadata(self):
        """Replace an existing report with escaped metadata and lossless image bytes."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            image = root / "snapshot.png"
            payload = b"\x89PNG\r\n\x1a\nexample-bytes"
            image.write_bytes(payload)
            output = root / "report.html"
            output.write_text("previous report")
            cases = [{"name": "<script>", "description": 'a & "b"',
                      "shots": [report.Shot(image, '<shot "one">', 12)],
                      "meta": {"value": "<unsafe>"}}]
            with patch.object(report, "HTML_REPORT", output):
                report._write_report(cases)
            html = output.read_text()
            self.assertIn("&lt;script&gt;", html)
            self.assertIn("a &amp; &quot;b&quot;", html)
            self.assertIn("&lt;unsafe&gt;", html)
            self.assertNotIn("<script>", html)
            self.assertIn(base64.b64encode(payload).decode("ascii"), html)
            self.assertTrue(html.rstrip().endswith("</html>"))
            self.assertEqual(list(root.glob(".cmux-report-*")), [])

    def test_oversized_snapshot_preserves_existing_report(self):
        """A screenshot beyond the bound leaves no partial report or temporary file."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            image = root / "oversized.png"
            with image.open("wb") as source:
                source.truncate(report.MAX_SNAPSHOT_BYTES + 1)
            output = root / "report.html"
            output.write_text("previous report")
            cases = [{"name": "case", "description": "oversized",
                      "shots": [report.Shot(image, "large", 0)]}]
            with patch.object(report, "HTML_REPORT", output):
                with self.assertRaisesRegex(ValueError, "Snapshot exceeds"):
                    report._write_report(cases)
            self.assertEqual(output.read_text(), "previous report")
            self.assertEqual(list(root.glob(".cmux-report-*")), [])

    def test_missing_snapshot_does_not_create_report(self):
        """A missing source propagates its file error and cleans the temporary output."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "report.html"
            cases = [{"name": "case", "description": "missing",
                      "shots": [report.Shot(root / "missing.png", "missing", 0)]}]
            with patch.object(report, "HTML_REPORT", output):
                with self.assertRaises(FileNotFoundError):
                    report._write_report(cases)
            self.assertFalse(output.exists())
            self.assertEqual(list(root.glob(".cmux-report-*")), [])


if __name__ == "__main__":
    unittest.main()
