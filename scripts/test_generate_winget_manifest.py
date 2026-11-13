"""Exercise first-time ARM64 addition and subsequent dual-architecture releases."""

import copy
from pathlib import Path
import random
import tempfile
import unittest
import uuid

import yaml

from generate_winget_manifest import generate, SNAPSHOT


class ManifestTests(unittest.TestCase):
    def installers(self, version='0.6.0'):
        return [{
            'Architecture': architecture,
            'InstallerUrl': f'https://example.com/v{version}/markview-{architecture}.msi',
            'InstallerSha256': ('A' if architecture == 'x64' else 'B') * 64,
            'ProductVersion': version,
            'ProductCode': '{' + str(uuid.uuid4()).upper() + '}',
            'UpgradeCode': '{1094AD43-1728-41AA-A880-31382AA58F5B}',
            'Publisher': 'szdytom',
            'DisplayName': 'markview',
        } for architecture in ('x64', 'arm64')]

    def test_first_arm64_addition_replaces_snapshot_installer_metadata(self):
        snapshot = {p: p.read_bytes() for p in SNAPSHOT.glob('*.yaml')}
        with tempfile.TemporaryDirectory() as output:
            installers = self.installers()
            destination = generate('0.6.0', installers, 'https://example.com/notes', output)
            self.assertEqual(destination.relative_to(output), Path('manifests/s/szdytom/Markview/0.6.0'))
            documents = {yaml.safe_load(p.read_text())['ManifestType']: yaml.safe_load(p.read_text())
                         for p in destination.glob('*.yaml')}
            self.assertEqual(set(documents), {'version', 'installer', 'defaultLocale'})
            for document in documents.values():
                self.assertEqual(document['PackageVersion'], '0.6.0')
                self.assertEqual(document['PackageIdentifier'], 'szdytom.Markview')
            manifest = documents['installer']
            self.assertEqual(manifest['InstallerType'], 'wix')
            self.assertEqual(manifest['Scope'], 'machine')
            self.assertEqual(manifest['FileExtensions'], ['md', 'markdown', 'mdown'])
            for actual, expected in zip(manifest['Installers'], installers, strict=True):
                for key in ('Architecture', 'InstallerUrl', 'InstallerSha256', 'ProductCode'):
                    self.assertEqual(actual[key], expected[key])
                self.assertEqual(actual['AppsAndFeaturesEntries'], [{
                    k: expected[k] for k in ('DisplayName', 'Publisher', 'ProductCode', 'UpgradeCode')}])
            locale = documents['defaultLocale']
            self.assertEqual(locale['ReleaseNotesUrl'], 'https://example.com/notes')
            self.assertIn('/v0.6.0/', locale['LicenseUrl'])
        self.assertEqual(snapshot, {p: p.read_bytes() for p in snapshot})

    def test_metadata_and_order_across_releases(self):
        rng = random.Random(64)
        with tempfile.TemporaryDirectory() as output:
            for _ in range(50):
                version = '.'.join(str(rng.randrange(100)) for _ in range(3))
                installers = self.installers(version)
                rng.shuffle(installers)
                for installer in installers:
                    installer['InstallerSha256'] = f'{rng.getrandbits(256):064X}'
                destination = generate(version, installers, 'https://example.com/notes', output)
                manifest = yaml.safe_load((destination / 'szdytom.Markview.installer.yaml').read_text())
                self.assertEqual([i['Architecture'] for i in manifest['Installers']], ['x64', 'arm64'])
                expected = {i['Architecture']: i for i in installers}
                for installer in manifest['Installers']:
                    source = expected[installer['Architecture']]
                    self.assertEqual(installer['InstallerSha256'], source['InstallerSha256'])
                    self.assertEqual(installer['ProductCode'], source['ProductCode'])

    def test_reject_incomplete_duplicate_or_unsupported_architectures(self):
        installers = self.installers()
        for invalid in ([], installers[:1], installers * 2,
                        [installers[0], installers[0]],
                        [installers[0], dict(installers[1], Architecture='x86')]):
            with self.subTest(installers=invalid), tempfile.TemporaryDirectory() as output:
                with self.assertRaises(ValueError):
                    generate('0.6.0', invalid, 'https://example.com/notes', output)
                self.assertEqual(list(Path(output).iterdir()), [])

    def test_reject_invalid_metadata_before_writing(self):
        mutations = [
            ('InstallerSha256', 'ABCD'), ('ProductCode', 'invalid'), ('UpgradeCode', 'invalid'),
            ('ProductVersion', '0.5.1'), ('Publisher', 'other'), ('DisplayName', 'other'),
        ]
        for architecture in (0, 1):
            for key, value in mutations:
                with self.subTest(architecture=architecture, field=key), tempfile.TemporaryDirectory() as output:
                    installers = self.installers()
                    installers[architecture][key] = value
                    with self.assertRaises(ValueError):
                        generate('0.6.0', installers, 'https://example.com/notes', output)
                    self.assertEqual(list(Path(output).iterdir()), [])

    def test_reject_shared_url_and_unstable_version(self):
        installers = self.installers()
        invalid = copy.deepcopy(installers)
        invalid[1]['InstallerUrl'] = invalid[0]['InstallerUrl']
        with tempfile.TemporaryDirectory() as output:
            with self.assertRaises(ValueError):
                generate('0.6.0', invalid, 'https://example.com/notes', output)
            for version in ('0.6.0-beta.1', '../escape', 'v0.6.0'):
                with self.subTest(version=version), self.assertRaises(ValueError):
                    generate(version, installers, 'https://example.com/notes', output)


if __name__ == '__main__':
    unittest.main()
