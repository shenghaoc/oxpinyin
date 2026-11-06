"""Oracle pin fields, image tag, image verification and runtime selection.

Standard library plus the shared tools.dev process helpers. Used by
capture-contract.sh, build-contract-image.sh and cloud provisioning:

    python -m tools.oracle.image image-tag
    python -m tools.oracle.image pin-field [--platform PLATFORM] KEY
    python -m tools.oracle.image verify-image [--engine docker|podman] [--platform PLATFORM] IMAGE
"""
import hashlib
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

if __name__ == '__main__' and __package__ in (None, ''):
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from tools.dev import cell
from tools.dev.process import ROOT, capture, fail, run, text
from tools.dev.runners import Finished, guarded

VERIFY_IMAGE_USAGE = '''Check a built oracle image against tools/oracle/oracle-pin.txt (issue #818).

For each cell (bdb, kc, tkrzw) the image's libpinyin.so and libzhuyin.so
must hash to the recorded recipe values, and the image must carry NO
model-derived data (docs/findings/model-provenance.md): no
lib/libpinyin/data directory and no *.bin, *.table, *.text or bigram.db
file anywhere under /opt/pinyin-oracle.
'''


def digest_file(path):
    """Return the hexadecimal SHA-256 of file bytes; read errors propagate."""
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def fields(path):
    """Read the oracle's key=value record without interpreting shell syntax."""
    return dict(line.split('=', 1) for line in Path(path).read_text().splitlines()
                if line and not line.startswith('#') and '=' in line)


def platform_field(pin, platform, key):
    """Read a frozen platform field; unsupported platforms have no value."""
    suffix = {'linux/amd64': '', 'linux/arm64': '_linux_arm64'}.get(platform)
    return '' if suffix is None else fields(pin).get(key + suffix, '')


def image_tag(root=ROOT):
    """Hash the original recipe inputs in their original byte order."""
    directory = Path(root) / 'tools/oracle'
    pin = directory / 'oracle-pin.txt'
    record = fields(pin)
    commit, base = record.get('libpinyin_commit', ''), record.get('base_image', '')
    if not commit or not base:
        fail(f'FAIL: {pin} lacks libpinyin_commit or base_image')
    if '@sha256:' in base:
        fail(f'FAIL: base_image {base} is digest-pinned; the catalog tracks a codename tag')
    if 'apt_snapshot' in record:
        fail(f'FAIL: {pin} still records an apt snapshot; snapshots are not pins')
    if base not in (directory / 'Dockerfile.oracle').read_text():
        fail(f'FAIL: Dockerfile.oracle does not use base_image {base}')
    digest = hashlib.sha256()
    for name in ('Dockerfile.oracle', 'build-oracle.sh', 'version-manifest.json', 'versions.py',
                 '../tkrzw/distro-probe.sh', '../lib/bash-floor.sh', 'oracle-pin.txt'):
        digest.update((directory / name).read_bytes())
    slug = re.sub(r'[^A-Za-z0-9_.-]', '-', base)
    return f'{commit[:12]}-{slug}-{digest.hexdigest()[:12]}'


def parse_options(argv, names, *, positional=False):
    """Parse the original image command grammar without abbreviations or defaults."""
    values, rest = {}, list(argv)
    while rest:
        option = rest.pop(0)
        if option in ('-h', '--help'):
            values['help'] = True
            return values
        if option in names:
            if not rest:
                fail(f'{option}: requires an argument', 1)
            values[option[2:]] = rest.pop(0)
        elif positional and not option.startswith('-'):
            values['positional'] = option
        else:
            fail(f'unknown option: {option}', 2)
    return values


def valid_platform(value):
    """Refuse platforms without a frozen oracle record."""
    if value not in ('linux/amd64', 'linux/arm64'):
        fail(f'unsupported platform: {value} (use linux/amd64 or linux/arm64)', 2)
    return value


PIN_FIELD_USAGE = 'usage: pin-field [--platform linux/amd64|linux/arm64] KEY'


def pin_field(argv, *, root=ROOT):
    """Print one platform-keyed field of oracle-pin.txt; an absent field prints nothing."""
    values = parse_options(argv, ('--platform',), positional=True)
    if values.get('help'):
        print(PIN_FIELD_USAGE)
        print('linux/arm64 reads KEY_linux_arm64; the unqualified fields are linux/amd64.')
        return 0
    key = values.get('positional')
    if not key:
        fail(PIN_FIELD_USAGE, 2)
    platform = valid_platform(values.get('platform', 'linux/amd64'))
    value = platform_field(Path(root) / 'tools/oracle/oracle-pin.txt', platform, key)
    if value:
        print(value)
    return 0


def verify_image(argv, *, root=ROOT):
    """Copy a scratch image's libraries, check frozen hashes and reject model data."""
    values = parse_options(argv, ('--engine', '--platform'), positional=True)
    if values.get('help'):
        print(VERIFY_IMAGE_USAGE, end='')
        return 0
    image = values.get('positional')
    if not image:
        fail('usage: verify-oracle-image.sh [--engine docker|podman] [--platform PLATFORM] IMAGE', 2)
    platform = valid_platform(values.get('platform', 'linux/amd64'))
    engine = values.get('engine') or next((e for e in ('docker', 'podman') if shutil.which(e)), '')
    if not engine:
        fail('neither docker nor podman found')
    cid = ''
    with tempfile.TemporaryDirectory() as scratch:
        try:
            created = capture([engine, 'create', '--pull=never', '--platform', platform, image, '/none'], check=False)
            cid = created.stdout.decode().rstrip('\n')
            if created.status:
                raise Finished(created.status)
            run([engine, 'cp', f'{cid}:/opt/pinyin-oracle', f'{scratch}/oracle'], stdout=subprocess.DEVNULL)
            status = 0
            for dbm in cell.CELLS:
                for library in ('libpinyin', 'libzhuyin'):
                    path = Path(scratch) / 'oracle' / dbm / 'lib' / (library + '.so')
                    try:
                        actual = digest_file(path)
                    except OSError as error:
                        print(f'sha256sum: {path}: {error.strerror}', file=sys.stderr)
                        raise Finished(1) from error
                    recorded = platform_field(Path(root) / 'tools/oracle/oracle-pin.txt', platform, f'{library}_sha256_{dbm}')
                    print(dbm, library, actual)
                    if not recorded or recorded != actual:
                        print(f'FAIL: {dbm} {library} is {actual}, oracle-pin.txt records {recorded or "nothing"}', file=sys.stderr)
                        status = 1
            leaks = [str(path) for path in (Path(scratch) / 'oracle').rglob('*')
                     if str(path).endswith('/lib/libpinyin/data') or path.name == 'bigram.db'
                     or path.suffix in ('.bin', '.table', '.text')]
            if leaks:
                print('FAIL: model-derived data in the image:\n' + '\n'.join(leaks), file=sys.stderr)
                status = 1
            return status
        finally:
            if cid:
                subprocess.run([engine, 'rm', '-f', cid], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def select_runtime(override='', *, env=None, host=None, output=print):
    """Preserve platform runtime order, explicit override, and unverified refusals."""
    environment = os.environ if env is None else env
    host = host or text(['uname', '-s'], env=environment)
    if host == 'Darwin':
        candidates = ['container', 'podman', 'docker']
    elif host.startswith(('MINGW', 'MSYS', 'CYGWIN')) or environment.get('WSL_DISTRO_NAME'):
        candidates = ['wslc', 'podman', 'docker']
    else:
        candidates = ['podman', 'docker']
    override = override or environment.get('OXPINYIN_CONTAINER', '')
    selected = override or next((name for name in candidates if shutil.which(name, path=environment.get('PATH'))), '')
    if not selected or not shutil.which(selected, path=environment.get('PATH')):
        fail(f'FAIL: no runtime available; candidates in order: {" ".join(candidates)} (override: {override or "none"})')
    if Path(selected).name == 'wslc':
        fail(f'Runtime: {selected}; version unavailable (adapter not yet verified)\n'
             'FAIL: wslc adapter not yet verified; select an exercised runtime explicitly with OXPINYIN_CONTAINER or --runtime')
    done = capture([selected, '--version'], env=environment, check=False)
    if done.status:
        fail(f'FAIL: cannot query runtime version: {selected}')
    output(f'Runtime: {selected}; {done.stdout.decode().rstrip(chr(10))}')
    return selected


def main(argv=None):
    """Dispatch image-tag, pin-field or verify-image; return the shell status."""
    args = list(sys.argv[1:] if argv is None else argv)

    def call():
        if args[:1] == ['image-tag']:
            print(image_tag())
            return 0
        if args[:1] == ['pin-field']:
            return pin_field(args[1:])
        if args[:1] == ['verify-image']:
            return verify_image(args[1:])
        print('usage: python -m tools.oracle.image image-tag|pin-field|verify-image ...', file=sys.stderr)
        return 2
    return guarded(call)


if __name__ == '__main__':
    sys.exit(main())
