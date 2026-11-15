"""Check Windows-only archive preparation without changing the checkout."""

from pathlib import Path
import tempfile
import tomllib
import unittest

from prepare_windows_archive import prepare


class PrepareWindowsArchiveTest(unittest.TestCase):
    def test_preparation_adds_only_the_helper_and_is_idempotent(self):
        original = Path(__file__).resolve().parents[1] / "dist-workspace.toml"
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory) / original.name
            config.write_bytes(original.read_bytes())
            expected = tomllib.loads(config.read_text(encoding="utf-8"))
            expected["dist"]["include"].append("packaging/windows/enable-portable.bat")
            prepare(config)
            self.assertEqual(tomllib.loads(config.read_text(encoding="utf-8")), expected)
            prepared = config.read_bytes()
            prepare(config)
            self.assertEqual(config.read_bytes(), prepared)


if __name__ == "__main__":
    unittest.main()
