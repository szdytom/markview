"""Run the system-window recovery regression tests on a host JVM."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from config import load

ROOT = Path(__file__).resolve().parent


class SystemWindowTest(unittest.TestCase):
    @unittest.skipUnless(shutil.which("javac") and shutil.which("java"), "requires a JDK")
    def test_launcher_recovery_and_deadlines(self):
        with tempfile.TemporaryDirectory() as directory:
            subprocess.run([
                "javac", "--release", load(ROOT.parent)["java-version"], "-Xlint:all", "-Werror", "-d", directory,
                str(ROOT / "tests/io/github/szdytom/markview/SystemWindowWait.java"),
                str(ROOT / "host-tests/SystemWindowWaitTest.java"),
            ], check=True)
            subprocess.run([
                "java", "-ea", "-cp", directory,
                "io.github.szdytom.markview.SystemWindowWaitTest",
            ], check=True)


if __name__ == "__main__":
    unittest.main()
