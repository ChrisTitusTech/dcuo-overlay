#!/usr/bin/env python3
"""Package a tested release binary. No game files or user data are included."""
import argparse
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
VERSION = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
DEPLOY_URL = 'https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage'
DEPLOY_SHA256 = '8aea8da0f7f7039d2a2cecb14657d752a222a5e1d3825caeef186c82f751cdd1'


def copy(source, destination):
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)


def stage(appdir, binary):
    copy(binary, appdir / 'usr/bin/dcuo-companion')
    copy(ROOT / 'assets/dcuo-companion.desktop', appdir / 'usr/share/applications/dcuo-companion.desktop')
    copy(ROOT / 'assets/dcuo-companion.svg', appdir / 'usr/share/icons/hicolor/scalable/apps/dcuo-companion.svg')
    for source in ('LICENSE', 'README.md', 'docs/release-0.1.md', 'docs/release-0.1.1.md'):
        copy(ROOT / source, appdir / 'usr/share/doc/dcuo-companion' / Path(source).name)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('format', choices=['rpm', 'appimage'])
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/release/dcuo-companion')
    parser.add_argument('--output', type=Path, default=ROOT / 'dist')
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='dcuo-package-') as temporary:
        work = Path(temporary)
        appdir = work / 'AppDir'
        stage(appdir, binary)
        if args.format == 'appimage':
            deploy = work / 'linuxdeploy.AppImage'
            with urllib.request.urlopen(DEPLOY_URL, timeout=60) as response:
                deploy.write_bytes(response.read(100 * 1024 * 1024))
            if hashlib.sha256(deploy.read_bytes()).hexdigest() != DEPLOY_SHA256:
                raise RuntimeError('linuxdeploy checksum mismatch; review pinned tool before updating')
            deploy.chmod(0o755)
            artifact = output / f'DCUO-Companion-{VERSION}-x86_64.AppImage'
            env = dict(os.environ, ARCH='x86_64', APPIMAGE_EXTRACT_AND_RUN='1',
                       OUTPUT=str(artifact), VERSION=VERSION)
            # Winit loads these at runtime, so ldd alone cannot discover them.
            libraries = []
            for name in ('libX11.so.6', 'libXcursor.so.1', 'libXi.so.6',
                         'libXrandr.so.2', 'libxkbcommon.so.0', 'libxkbcommon-x11.so.0'):
                candidates = [Path(prefix) / name for prefix in
                              ('/usr/lib/x86_64-linux-gnu', '/usr/lib64', '/lib64')]
                library = next((path for path in candidates if path.is_file()), None)
                if library is None:
                    raise RuntimeError(f'Missing runtime library: {name}')
                libraries += ['--library', str(library)]
            subprocess.run([str(deploy), '--appdir', str(appdir),
                            '--executable', str(appdir / 'usr/bin/dcuo-companion'),
                            '--desktop-file', str(ROOT / 'assets/dcuo-companion.desktop'),
                            '--icon-file', str(ROOT / 'assets/dcuo-companion.svg'),
                            '--output', 'appimage', *libraries], env=env, cwd=work, check=True)
            if not artifact.is_file():
                raise RuntimeError('AppImage output missing')
        else:
            top = work / 'rpmbuild'
            for directory in ('BUILD', 'BUILDROOT', 'RPMS', 'SOURCES', 'SPECS', 'SRPMS'):
                (top / directory).mkdir(parents=True, exist_ok=True)
            spec = f'''Name: dcuo-companion
Version: {VERSION}
Release: 1
Summary: DC Universe Online companion
License: GPL-3.0-only
URL: https://github.com/ChrisTitusTech/dcuo-overlay
BuildArch: x86_64
Requires: glibc >= 2.35, libgcc, libX11, libXcursor, libXi, libXrandr, libxkbcommon, libxkbcommon-x11, mesa-libGL
Recommends: tesseract, tesseract-langpack-eng
AutoReqProv: no
%description
Read-only game information, local maps, feats and optional Broker tools.
%install
mkdir -p %{{buildroot}}
cp -a "{appdir}/usr" %{{buildroot}}/
%files
/usr/bin/dcuo-companion
/usr/share/applications/dcuo-companion.desktop
/usr/share/icons/hicolor/scalable/apps/dcuo-companion.svg
%doc /usr/share/doc/dcuo-companion/README.md
%doc /usr/share/doc/dcuo-companion/release-0.1.md
%doc /usr/share/doc/dcuo-companion/release-0.1.1.md
%license /usr/share/doc/dcuo-companion/LICENSE
'''
            spec_path = top / 'SPECS/dcuo-companion.spec'
            spec_path.write_text(spec)
            subprocess.run(['rpmbuild', '-bb', '--define', f'_topdir {top}',
                            '--define', '__os_install_post %{nil}', str(spec_path)], check=True)
            results = list((top / 'RPMS').rglob('*.rpm'))
            if len(results) != 1:
                raise RuntimeError('Expected exactly one RPM')
            artifact = output / results[0].name
            copy(results[0], artifact)
        print(artifact)


if __name__ == '__main__':
    main()
