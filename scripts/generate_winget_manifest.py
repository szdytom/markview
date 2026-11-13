"""Generate a dual-architecture manifest from inspected MSI metadata."""

import argparse
import json
from pathlib import Path
import re

import yaml

SNAPSHOT = Path(__file__).resolve().parent.parent / 'packaging/winget/0.1.10'


def generate(version, installers, release_url, output):
    if not re.fullmatch(r'\d+\.\d+\.\d+', version):
        raise ValueError('expected a stable package version')
    if len(installers) != 2 or {i['Architecture'] for i in installers} != {'x64', 'arm64'}:
        raise ValueError('expected exactly one x64 and one arm64 installer')
    for installer in installers:
        if not re.fullmatch(r'[0-9A-Fa-f]{64}', installer['InstallerSha256']):
            raise ValueError('invalid installer SHA-256')
        for key in ('ProductCode', 'UpgradeCode'):
            if not re.fullmatch(r'\{[0-9A-Fa-f]{8}(?:-[0-9A-Fa-f]{4}){3}-[0-9A-Fa-f]{12}\}', installer[key]):
                raise ValueError(f'invalid MSI {key}')
        if installer['ProductVersion'] != version:
            raise ValueError('MSI version differs from package version')
        if installer['Publisher'] != 'szdytom' or installer['DisplayName'] != 'markview':
            raise ValueError('unexpected MSI publisher or product name')
    if installers[0]['InstallerUrl'] == installers[1]['InstallerUrl']:
        raise ValueError('architectures must use separate MSI URLs')

    manifests = {}
    for source in SNAPSHOT.glob('*.yaml'):
        data = yaml.safe_load(source.read_text())
        data['PackageVersion'] = version
        if data['ManifestType'] == 'installer':
            data['Installers'] = []
            for installer in sorted(installers, key=lambda i: i['Architecture'], reverse=True):
                entry = {k: installer[k] for k in (
                    'Architecture', 'InstallerUrl', 'InstallerSha256', 'ProductCode')}
                entry['AppsAndFeaturesEntries'] = [{
                    'DisplayName': installer['DisplayName'],
                    'Publisher': installer['Publisher'],
                    'ProductCode': installer['ProductCode'],
                    'UpgradeCode': installer['UpgradeCode'],
                }]
                data['Installers'].append(entry)
        elif data['ManifestType'] == 'defaultLocale':
            data['ReleaseNotesUrl'] = release_url
            data['LicenseUrl'] = data['LicenseUrl'].replace('/v0.1.10/', f'/v{version}/')
        manifests[source.name] = data

    destination = Path(output) / 'manifests/s/szdytom/Markview' / version
    destination.mkdir(parents=True, exist_ok=True)
    for name, data in manifests.items():
        schema = data['ManifestType']
        header = f'# yaml-language-server: $schema=https://aka.ms/winget-manifest.{schema}.1.10.0.schema.json\n'
        (destination / name).write_text(header + yaml.safe_dump(data, sort_keys=False), encoding='utf-8')
    return destination


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('metadata', type=Path)
    parser.add_argument('--version', required=True)
    parser.add_argument('--release-url', required=True)
    parser.add_argument('--out', default='.')
    args = parser.parse_args()
    print(generate(args.version, json.loads(args.metadata.read_text(encoding='utf-8-sig')),
                   args.release_url, args.out))
