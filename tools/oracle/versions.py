#!/usr/bin/env python3
"""Record and gate the pin-reference image's tool versions.

The accepted versions live in version-manifest.json beside this file. Each
oracle and compatibility image build runs ``--check``: it prints the
installed package version, the range basis and the verdict, writes the same
record into the image, and exits non-zero naming the first component outside
its range. Rollin testing images run ``--print``, which records the
versions without a range check.

Standard library only. The manifest and this file travel into
tools/oracle/Dockerfile.oracle and tools/oracle/Dockerfile.compat; the
``--probe`` option points at tools/tkrzw/distro-probe.sh.
"""
from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

DEFAULT_MANIFEST = Path(__file__).with_name('version-manifest.json')
DEFAULT_PROBE = 'tools/tkrzw/distro-probe.sh'
SCHEMA = 'oracle-version-manifest-v1'
# Print-only fallbacks for rolling images, where a package may be renamed or a
# compiler metapackage may be absent. Checked images use the package version;
# these commands only keep the print record useful on Debian testing.
PRINT_FALLBACKS = {
    'glib': ('pkg-config', '--modversion', 'glib-2.0'),
    'C compiler': ('gcc', '--version'),
    'C++ compiler': ('g++', '--version'),
    'autoconf': ('autoconf', '--version'),
    'automake': ('automake', '--version'),
    'libtool': ('libtool', '--version'),
    'gettext': ('gettext', '--version'),
    'pkg-config': ('pkg-config', '--version'),
}


def numeric_prefix(version):
    """Return the leading (major, minor) of a Debian version.

    Strips an epoch, then reads the first two dotted integers. Debian
    suffixes such as ``-12+deb13u4``, ``+dfsg2-9`` and ``~deb13u5`` do not
    affect the major.minor.
    """
    text = re.sub(r'^[0-9]+:', '', version.strip())
    match = re.match(r'([0-9]+)(?:\.([0-9]+))?', text)
    if match is None:
        raise ValueError(f'no numeric version prefix in {version!r}')
    return int(match.group(1)), int(match.group(2) or 0)


def major_minor(version):
    """Return the version's major.minor as the manifest spells it."""
    major, minor = numeric_prefix(version)
    return f'{major}.{minor}'


def package_version(package):
    """Return the installed Debian package version, or raise LookupError."""
    try:
        result = subprocess.run(
            ['dpkg-query', '-W', '-f=${Version}', package],
            check=False, capture_output=True, text=True)
    except FileNotFoundError as error:
        raise LookupError('dpkg-query is unavailable outside a Debian image') from error
    version = result.stdout.strip()
    if result.returncode != 0 or not version:
        detail = result.stderr.strip() or 'not installed'
        raise LookupError(f'{package}: {detail}')
    return version


def command_version(command):
    """Return the first dotted version a command reports, or raise LookupError."""
    try:
        result = subprocess.run(command, check=False, capture_output=True, text=True)
    except FileNotFoundError as error:
        raise LookupError(f'{command[0]} is unavailable') from error
    if result.returncode != 0:
        raise LookupError(f'{command[0]} exited {result.returncode}')
    match = re.search(r'[0-9]+(?:\.[0-9]+)+', result.stdout + ' ' + result.stderr)
    if match is None:
        raise LookupError(f'{command[0]} reported no version')
    return match.group(0)


def print_version(component):
    """Return the installed package version or a command fallback for --print."""
    try:
        return package_version(component['package'])
    except LookupError as error:
        fallback = PRINT_FALLBACKS.get(component['name'])
        if fallback is None:
            raise
        try:
            return command_version(fallback)
        except LookupError:
            raise error


def load_manifest(path):
    """Read and validate the pinned version manifest."""
    with Path(path).open(encoding='utf-8') as source:
        manifest = json.load(source)
    if manifest.get('schema') != SCHEMA:
        raise ValueError(f'{path}: schema is {manifest.get("schema")!r}, expected {SCHEMA!r}')
    components = manifest.get('components')
    if not isinstance(components, list) or not components:
        raise ValueError(f'{path}: no components')
    names = set()
    for component in components:
        name = component.get('name')
        if not name or name in names:
            raise ValueError(f'{path}: missing or duplicate component name {name!r}')
        names.add(name)
        if not component.get('package'):
            raise ValueError(f'{path}: {name} has no package')
        if major_minor(component.get('major_minor', '')) != component['major_minor']:
            raise ValueError(f'{path}: {name} range is not a major.minor: {component.get("major_minor")!r}')
        if not component.get('basis'):
            raise ValueError(f'{path}: {name} has no range basis')
    return manifest


def check(manifest, resolve=package_version, probe=DEFAULT_PROBE, record=None):
    """Print installed versions and return 1 if any check fails.

    ``resolve`` and ``probe`` are injectable for the unit tests. A component
    outside its major.minor is a failure, and the tkrzw component
    additionally runs the distribution probe.
    """
    lines = []
    failures = []
    for component in manifest['components']:
        name = component['name']
        package = component['package']
        try:
            version = resolve(package)
        except LookupError as error:
            failures.append(f'{name}: {error}')
            continue
        actual = major_minor(version)
        expected = component['major_minor']
        verdict = 'OK' if actual == expected else 'FAIL'
        lines.append(f'{name}\t{package}\t{version}\t{expected}\t{verdict}')
        if verdict != 'OK':
            failures.append(
                f'{name}: {package} is {version} (major.minor {actual}), '
                f'manifest accepts {expected}')
    if any(component.get('probe') for component in manifest['components']):
        result = subprocess.run(['sh', probe], check=False, capture_output=True, text=True)
        if result.stdout:
            print(result.stdout, end='')
        if result.stderr:
            print(result.stderr, end='', file=sys.stderr)
        if result.returncode != 0:
            failures.append(f'tkrzw: distro probe {probe} exited {result.returncode}')
    for line in lines:
        print(line)
    if record is not None:
        record.write_text(''.join(line + '\n' for line in lines), encoding='utf-8')
    for failure in failures:
        print(f'FAIL: {failure}', file=sys.stderr)
    return 1 if failures else 0


def main(argv=None):
    """Parse arguments, run the print or check mode, and return the status."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--manifest', type=Path, default=DEFAULT_MANIFEST,
                        help='version manifest (default: beside this file)')
    parser.add_argument('--probe', default=DEFAULT_PROBE,
                        help='tkrzw distribution probe (default: %(default)s)')
    parser.add_argument('--record', type=Path,
                        help='also write the printed version lines to this file')
    parser.add_argument('--print', dest='print_only', action='store_true',
                        help='print installed versions without checking ranges')
    args = parser.parse_args(argv)
    try:
        manifest = load_manifest(args.manifest)
    except (OSError, ValueError) as error:
        print(f'FAIL: {error}', file=sys.stderr)
        return 2
    if args.print_only:
        lines = []
        for component in manifest['components']:
            try:
                installed = print_version(component)
            except LookupError as error:
                # Print mode is a record, not a gate: a rolling testing image
                # may rename a package, and the build does not check ranges.
                installed = f'MISSING ({error})'
            lines.append(f'{component["name"]}\t{component["package"]}\t{installed}')
        text = ''.join(line + '\n' for line in lines)
        sys.stdout.write(text)
        if args.record:
            args.record.write_text(text, encoding='utf-8')
        return 0
    return check(manifest, probe=args.probe, record=args.record)


if __name__ == '__main__':
    sys.exit(main())
