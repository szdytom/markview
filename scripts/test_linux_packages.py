"""Exercise Linux package contents and architecture selection with real scripts."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
APPIMAGETOOL = r'''
import json
import os
from pathlib import Path
import subprocess
import sys

assert sys.argv[1] == "--appimage-extract-and-run"
appdir, output = map(Path, sys.argv[2:])
for name in ("markview.desktop", "markview.png",
             "usr/share/applications/markview.desktop",
             "usr/share/doc/markview/THIRD_PARTY.md",
             "usr/share/doc/markview/third-party-notices.html"):
    assert (appdir / name).is_file(), name
assert subprocess.check_output([str(appdir / "AppRun"), "--help"], text=True) == "markview fixture\n"
Path(os.environ["PACKAGE_TRACE"]).write_text(json.dumps(dict(
    arch=os.environ["ARCH"], version=os.environ["VERSION"], output=output.name)))
output.write_text("AppImage fixture")
'''


class LinuxPackagesTest(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory(prefix="markview packages ")
        self.addCleanup(directory.cleanup)
        self.directory = Path(directory.name)
        self.binary = self.directory / "markview"
        self.binary.write_text('#!/bin/sh\nprintf "markview fixture\\n"\n')
        self.binary.chmod(0o755)
        self.notices = self.directory / "notices.html"
        self.notices.write_text("Third-party licenses")
        self.env = dict(os.environ, MARKVIEW_NOTICES=str(self.notices))

    def package(self, script, *args):
        return subprocess.run(["bash", str(ROOT / "scripts" / script), str(self.binary),
                               "1.2.3", *map(str, args)], env=self.env,
                              capture_output=True, text=True, check=True)

    @unittest.skipUnless(shutil.which("dpkg-deb"), "Requires dpkg-deb")
    def test_debian_architectures_ship_the_required_loaders_and_resources(self):
        for arch in ("amd64", "arm64", "armhf"):
            with self.subTest(arch=arch):
                out = self.directory / arch
                self.package("package_deb.sh", arch, out)
                deb = out / f"markview_1.2.3_{arch}.deb"
                field = lambda name: subprocess.check_output(
                    ["dpkg-deb", "--field", str(deb), name], text=True).strip()
                self.assertEqual(field("Architecture"), arch)
                self.assertEqual(field("Version"), "1.2.3")
                dependencies = field("Depends").split(", ")
                self.assertIn("libc6 (>= 2.35)", dependencies)
                self.assertIn("libvulkan1", dependencies)
                for loader in ("libegl1", "libgles2"):
                    self.assertEqual(loader in dependencies, arch != "amd64")
                extracted = out / "extracted"
                subprocess.run(["dpkg-deb", "--extract", str(deb), str(extracted)], check=True)
                self.assertEqual(subprocess.check_output(
                    [str(extracted / "usr/bin/markview"), "--help"], text=True), "markview fixture\n")
                self.assertEqual((extracted / "usr/share/doc/markview/third-party-notices.html").read_text(),
                                 self.notices.read_text())
                for name in ("usr/share/applications/markview.desktop",
                             "usr/share/icons/hicolor/512x512/apps/markview.png"):
                    self.assertGreater((extracted / name).stat().st_size, 0)

    def test_appimage_uses_the_matching_tool_architecture_and_filename(self):
        tool = self.directory / "appimagetool"
        tool.write_text(f"#!{sys.executable}\n" + APPIMAGETOOL)
        tool.chmod(0o755)
        trace = self.directory / "trace.json"
        self.env.update(APPIMAGETOOL=str(tool), PACKAGE_TRACE=str(trace))
        for arch in ("x86_64", "aarch64"):
            with self.subTest(arch=arch):
                out = self.directory / arch
                self.package("package_appimage.sh", out, arch)
                name = f"markview-1.2.3-{arch}.AppImage"
                self.assertTrue((out / name).is_file())
                self.assertEqual(json.loads(trace.read_text()),
                                 dict(arch=arch, version="1.2.3", output=name))

    def test_appimage_downloads_the_native_tool_by_default(self):
        tools = self.directory / "bin"
        tools.mkdir()
        tool = self.directory / "appimagetool"
        tool.write_text(f"#!{sys.executable}\n" + APPIMAGETOOL)
        tool.chmod(0o755)
        (tools / "curl").write_text(f"#!{sys.executable}\n" + r'''
import os
from pathlib import Path
import shutil
import sys
Path(os.environ["DOWNLOAD_TRACE"]).write_text(sys.argv[-1])
shutil.copyfile(os.environ["PACKAGE_TOOL"], sys.argv[sys.argv.index("--output") + 1])
''')
        (tools / "curl").chmod(0o755)
        download = self.directory / "download.txt"
        trace = self.directory / "trace.json"
        self.env.update(PATH=str(tools) + os.pathsep + os.environ["PATH"],
                        PACKAGE_TOOL=str(tool), DOWNLOAD_TRACE=str(download), PACKAGE_TRACE=str(trace))
        self.env.pop("APPIMAGETOOL", None)
        for arch in ("x86_64", "aarch64"):
            with self.subTest(arch=arch):
                uname = tools / "uname"
                uname.write_text(f'#!/bin/sh\nprintf "{arch}\\n"\n')
                uname.chmod(0o755)
                self.package("package_appimage.sh", self.directory / arch)
                self.assertEqual(download.read_text(),
                                 f"https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-{arch}.AppImage")
                self.assertEqual(json.loads(trace.read_text())["arch"], arch)


if __name__ == "__main__":
    unittest.main()
