"""Exercise release workflow commands without contacting Marketplace."""

import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import tempfile
import textwrap
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[1]
PLATFORMS = ('darwin-arm64', 'win32-x64', 'win32-arm64',
             'linux-x64', 'linux-arm64')


def command(workflow, step):
    section = (ROOT / '.github/workflows' / workflow).read_text().split(f'- name: {step}\n')[1]
    run = re.search(r'^        run: (.+)\n', section, re.M)
    if run[1] != '|':
        return run[1]
    block = re.match(r'(?:          .*\n|\n)+', section[run.end():])[0]
    return textwrap.dedent(block)


class ExtensionReleaseTest(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        (self.root / 'scripts').mkdir()
        shutil.copy(ROOT / 'scripts/verify_extension_packages.py', self.root / 'scripts')
        extension = self.root / 'web/app/vsc-ext/vscode'
        extension.mkdir(parents=True)
        self.manifest = json.loads((ROOT / 'web/app/vsc-ext/vscode/package.json').read_text())
        (extension / 'package.json').write_text(json.dumps(self.manifest))
        (self.root / 'vsix').mkdir()
        for platform in PLATFORMS:
            self.package(platform)
        tools = self.root / 'bin'
        tools.mkdir()
        gh = tools / 'gh'
        gh.write_text('#!/bin/sh\ncat "$RUNNER_TEMP/release-fixture.json"\nexit "${GH_STATUS:-0}"\n')
        gh.chmod(0o755)
        vsce = self.root / 'web/app/vsc-ext/node_modules/.bin/vsce'
        vsce.parent.mkdir(parents=True)
        vsce.write_text('#!/bin/sh\nprintf "%s\\n" "$@" > "$RUNNER_TEMP/published"\n')
        vsce.chmod(0o755)
        self.env = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ['PATH'],
                        RUNNER_TEMP=str(self.root), GITHUB_OUTPUT=str(self.root / 'output'),
                        RELEASE_TAG='v9.8.7', RELEASE_PLAN='', GITHUB_REPOSITORY='szdytom/markview')
        self.stage = command('packaging.yml', 'Verify the complete extension release')
        self.verify = command('extension-release.yml', 'Verify the released packages before authenticating')
        self.publish = command('extension-release.yml', 'Publish the released packages with Trusted Publishing')

    def package(self, platform, version=None):
        data = bytearray(512)
        arm = platform.endswith('arm64')
        if platform.startswith('darwin'):
            struct.pack_into('<II', data, 0, 0xfeedfacf, 0x100000c if arm else 0x1000007)
        elif platform.startswith('linux'):
            data[:6] = b'\x7fELF\x02\x01'
            struct.pack_into('<H', data, 18, 183 if arm else 62)
        else:
            data[:2] = b'MZ'
            struct.pack_into('<I', data, 60, 64)
            data[64:68] = b'PE\0\0'
            struct.pack_into('<H', data, 68, 0xaa64 if arm else 0x8664)
            struct.pack_into('<H', data, 88, 0x20b)
        name = f"{self.manifest['name']}-{self.manifest['version']}-{platform}.vsix"
        file = self.root / 'vsix' / name
        with zipfile.ZipFile(file, 'w') as archive:
            package = dict(self.manifest, version=version or self.manifest['version'])
            archive.writestr('extension/package.json', json.dumps(package))
            archive.writestr('extension.vsixmanifest', '<PackageManifest xmlns="urn:vsix">'
                            f'<Metadata><Identity TargetPlatform="{platform}"/></Metadata></PackageManifest>')
            archive.writestr('extension/bin/markview' + ('.exe' if platform.startswith('win32') else ''), data)
        return file

    def run_shell(self, script, **env):
        return subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', script], cwd=self.root,
                              env=dict(self.env, **env), capture_output=True, text=True, timeout=20)

    def test_release_resolution_accepts_only_published_stable_matching_tags(self):
        resolve = command('extension-release.yml', 'Resolve the published stable release')
        for change, status, ok in (({}, '0', True), ({'draft': True}, '0', False),
                                   ({'prerelease': True}, '0', False),
                                   ({'tag_name': 'v9.8.7-beta.1'}, '0', False),
                                   ({'tag_name': 'v9.8.6'}, '0', False), ({}, '1', False)):
            with self.subTest(change=change, status=status):
                fixture = dict(draft=False, prerelease=False, tag_name='v9.8.7')
                fixture.update(change)
                (self.root / 'release-fixture.json').write_text(json.dumps(fixture))
                output = self.root / 'output'
                output.unlink(missing_ok=True)
                result = self.run_shell(resolve, GH_STATUS=status,
                                        RELEASE_PLAN=json.dumps({'announcement_tag': 'v9.8.7'}),
                                        RELEASE_TAG='ignored')
                self.assertEqual(result.returncode == 0, ok, result.stderr)
                self.assertEqual(output.exists(), ok)
                if ok:
                    self.assertEqual(output.read_text(), 'tag=v9.8.7\n')
        (self.root / 'release-fixture.json').write_text(json.dumps(
            dict(draft=False, prerelease=False, tag_name='v9.8.7')))
        self.assertEqual(self.run_shell(resolve).returncode, 0)

    def test_staged_packages_publish_without_rebuilding_or_version_changes(self):
        result = self.run_shell(self.stage)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len((self.root / 'vsix/SHA256SUMS-vscode').read_text().splitlines()), 5)
        result = self.run_shell(self.verify + '\n' + self.publish)
        self.assertEqual(result.returncode, 0, result.stderr)
        args = (self.root / 'published').read_text().splitlines()
        self.assertEqual(args[:3], ['publish', '--oidc', '--packagePath'])
        self.assertEqual(args[3:-1], sorted(str(path.relative_to(self.root))
                                          for path in (self.root / 'vsix').glob('*.vsix')))
        self.assertEqual(args[-1], '--skip-duplicate')

    def test_missing_mismatched_or_changed_packages_cannot_publish(self):
        for defect in ('missing', 'version', 'changed'):
            with self.subTest(defect=defect):
                file = self.package('win32-arm64')
                self.assertEqual(self.run_shell(self.stage).returncode, 0)
                if defect == 'missing':
                    file.unlink()
                elif defect == 'version':
                    self.package('win32-arm64', version='0.0.1')
                    self.assertNotEqual(self.run_shell(self.stage).returncode, 0)
                else:
                    with file.open('ab') as output:
                        output.write(b'changed after release')
                result = self.run_shell(self.verify + '\n' + self.publish)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse((self.root / 'published').exists())

    def test_intel_macos_package_is_not_part_of_the_release(self):
        self.package('darwin-x64')
        result = self.run_shell(self.stage)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('darwin-x64', result.stderr)
        self.assertFalse((self.root / 'vsix/SHA256SUMS-vscode').exists())


if __name__ == '__main__':
    unittest.main()
