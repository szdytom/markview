"""Verify release package identity, version and native architecture before publishing."""
import json
import pathlib
import struct
import sys
import zipfile
import xml.etree.ElementTree as ET

manifest = json.loads(pathlib.Path('web/app/vsc-ext/vscode/package.json').read_text())
platforms = {'darwin-arm64', 'darwin-x64', 'linux-arm64', 'linux-x64', 'win32-arm64', 'win32-x64'}
found = set()
for file in pathlib.Path(sys.argv[1]).glob('*.vsix'):
    with zipfile.ZipFile(file) as archive:
        package = json.loads(archive.read('extension/package.json'))
        for key in ('name', 'publisher', 'version'):
            assert package[key] == manifest[key], (file, key)
        identity = next(node for node in ET.fromstring(archive.read('extension.vsixmanifest')).iter()
                        if node.tag.endswith('}Identity'))
        platform = identity.attrib['TargetPlatform']
        assert platform in platforms and platform not in found, platform
        found.add(platform)
        name = 'markview.exe' if platform.startswith('win32') else 'markview'
        data = archive.read('extension/bin/' + name)
        arm = platform.endswith('arm64')
        if platform.startswith('win32'):
            offset = struct.unpack_from('<I', data, 60)[0]
            assert data[:2] == b'MZ' and data[offset:offset+4] == b'PE\0\0'
            assert struct.unpack_from('<H', data, offset+4)[0] == (0xaa64 if arm else 0x8664)
        elif platform.startswith('darwin'):
            assert struct.unpack_from('<I', data)[0] == 0xfeedfacf
            assert struct.unpack_from('<I', data, 4)[0] == (0x100000c if arm else 0x1000007)
        else:
            assert data[:6] == b'\x7fELF\x02\x01'
            assert struct.unpack_from('<H', data, 18)[0] == (183 if arm else 62)
assert found == platforms, f'Missing platforms: {platforms - found}'
print('Verified all six Markview Integration packages')
