#!/usr/bin/env python3
"""Record installed component versions in rolling Debian images.

Version drift is recorded, not rejected. The default mode fails on a missing
package; --print records capture dependencies and marks a missing one instead.
The images use Debian's libtkrzw, which is sound; Ubuntu's packaged libtkrzw
is not (docs/findings/tkrzw-distro-compat.md).
"""
from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

DEFAULT_MANIFEST = Path(__file__).with_name('version-manifest.json')
SCHEMA = 'oracle-version-manifest-v1'
# Command fallbacks for capture images, where build-only packages can be absent.
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
    """Read and validate the component inventory."""
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
        if not component.get('basis'):
            raise ValueError(f'{path}: {name} has no basis')
    return manifest


def check(manifest, resolve=package_version, record=None):
    """Record installed versions; missing packages fail."""
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
        lines.append(f'{name}\t{package}\t{version}')
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
    parser.add_argument('--record', type=Path,
                        help='also write the printed version lines to this file')
    parser.add_argument('--print', dest='print_only', action='store_true',
                        help='record installed versions, marking a missing package instead of failing')
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
    return check(manifest, record=args.record)


if __name__ == '__main__':
    sys.exit(main())
