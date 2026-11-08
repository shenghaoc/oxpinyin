"""Oracle pin fields, image tag, image verification and runtime selection.

Standard library plus the shared tools.dev process helpers. Used by
capture-image.sh, capture-contract.sh and cloud provisioning:

    python -m tools.oracle.image image-tag
    python -m tools.oracle.image pin-field [--platform PLATFORM] KEY
    python -m tools.oracle.image verify-image [--engine docker|podman|wslc] [--platform PLATFORM] IMAGE
    python -m tools.oracle.image build-image
    python -m tools.oracle.image image-field ENGINE FIELD IMAGE

docker and podman take each command as written. wslc (WSL containers, Windows) has no
--platform, no cp and no rm, and its image inspect prints only JSON; wslc_command and
the wslc branches below translate those calls (docs/runbooks/setup.md, native Windows).
"""
import json
import argparse
import hashlib
import os
import platform
import platform as platform_module
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile

if __name__ == '__main__' and __package__ in (None, ''):
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from tools.dev import cell
from tools.dev.process import ROOT, capture, fail, run, text
from tools.dev.runners import Finished, guarded

VERIFY_IMAGE_USAGE = '''Check a built oracle image against tools/oracle/oracle-pin.txt (issue #818).

For each cell (bdb, kc, tkrzw) the image's libpinyin.so and libzhuyin.so
must match their own recorded hashes and the current pin/recipe, and the image must carry NO
model-derived data (docs/findings/model-provenance.md): no
lib/libpinyin/data directory and no *.bin, *.table, *.text or bigram.db
file anywhere under /opt/pinyin-oracle.

Every image runs at the host's architecture, with no emulation: PLATFORM defaults to
the host's (linux/amd64 on x86_64, linux/arm64 on aarch64), and any other is refused.
'''


ARCHITECTURES = {'x86_64': 'amd64', 'AMD64': 'amd64', 'aarch64': 'arm64', 'arm64': 'arm64', 'ARM64': 'arm64'}
REGISTRY = 'ghcr.io/shenghaoc/oxpinyin-oracle'


def host_system():
    """The host OS as platform.system() names it: Linux, Darwin or Windows."""
    return platform.system()


def is_wslc(engine):
    """True for the WSL containers CLI, by any path or with its .exe suffix.

    On Windows wslc also ships as container.exe, so container and container.exe count there."""
    name = Path(str(engine)).name.lower()
    return name in ('wslc', 'wslc.exe') or (name in ('container', 'container.exe') and host_system() == 'Windows')


def wslc_command(command):
    """Translate one docker-grammar command for wslc; other engines pass through unchanged.

    wslc runs only the host architecture and has no --platform, so the flag and its value are
    dropped (callers check the host architecture first). rm is remove there."""
    if not is_wslc(command[0]):
        return list(command)
    translated, rest = [command[0]], list(command[1:])
    if rest[:1] == ['rm']:
        rest[0] = 'remove'
    while rest:
        part = rest.pop(0)
        if part == '--platform':
            rest.pop(0)
        elif not str(part).startswith('--platform='):
            translated.append(part)
    return translated


def host_platform():
    """The linux/ platform of this host's architecture, or None for another architecture."""
    architecture = ARCHITECTURES.get(platform.machine())
    return architecture and 'linux/' + architecture


def image_field(engine, field, image, *, env=None):
    """One top-level field of an image's inspect record: Go template, or wslc's JSON."""
    if is_wslc(engine):
        done = capture([engine, 'image', 'inspect', '--format', 'json', image], env=env)
        return str(json.loads(done.stdout.decode())[0][field])
    return text([engine, 'image', 'inspect', '--format', '{{.' + field + '}}', image], env=env)


def digest_file(path):
    """Return the hexadecimal SHA-256 of file bytes; read errors propagate."""
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def fields(path):
    """Read the oracle's key=value record without interpreting shell syntax."""
    return dict(line.split('=', 1) for line in Path(path).read_text().splitlines()
                if line and not line.startswith('#') and '=' in line)


def platform_field(pin, platform, key):
    """Read a platform input field; unsupported platforms have no value."""
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
    slug = re.sub(r'[^A-Za-z0-9_.-]', '-', base)
    return f'{commit[:12]}-{slug}-{recipe_hash(root)[:12]}'


def recipe_hash(root=ROOT):
    """Hash the recipe inputs in the same order as build-oracle.sh."""
    directory = Path(root) / 'tools/oracle'
    digest = hashlib.sha256()
    for name in ('Dockerfile.oracle', 'build-oracle.sh', 'version-manifest.json', 'versions.py',
                 '../lib/bash-floor.sh', 'oracle-pin.txt'):
        digest.update((directory / name).read_bytes())
    return digest.hexdigest()


def prefix_ok(prefix, backend, root=ROOT, pin=None, expected_platform=None):
    """Validate current pin/recipe provenance and the prefix's recorded bytes."""
    prefix = Path(prefix)
    try:
        expected = fields(pin or Path(root) / 'tools/oracle/oracle-pin.txt')
        record = fields(prefix / 'oracle-pin.txt')
        if any(record.get(key) != expected[key] for key in ('libpinyin_commit', 'ibus_libpinyin_commit')):
            return False
        if record.get('recipe_sha256') != recipe_hash(root) or record.get('dbm') != cell.BACKEND_FORMATS[cell.CELL_TO_BACKEND[backend]]:
            return False
        architecture = {'x86_64': 'linux/amd64', 'AMD64': 'linux/amd64',
                        'arm64': 'linux/arm64', 'aarch64': 'linux/arm64'}
        wanted = expected_platform or architecture.get(platform.machine())
        if wanted is None or architecture.get(record.get('architecture')) != wanted:
            return False
        if record.get('patches_manifest_sha256'):
            return False
        for library, key in (('libpinyin', 'shared_object_sha256'), ('libzhuyin', 'zhuyin_shared_object_sha256')):
            if digest_file(prefix / 'lib' / (library + '.so')) != record.get(key):
                return False
        return digest_file(prefix / 'versions.txt') == record.get('versions_sha256')
    except (OSError, KeyError, ValueError):
        return False


def model_ok(prefix, root=ROOT, pin=None):
    """The prefix's recorded model20 archive is the repository's pinned one."""
    expected = fields(pin or Path(root) / 'tools/oracle/oracle-pin.txt')
    return fields(Path(prefix) / 'oracle-pin.txt').get('model_sha256') == expected.get('model_archive_sha256')


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
    """Refuse unsupported oracle platforms."""
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
    """Copy a scratch image and check build provenance and absence of model data."""
    values = parse_options(argv, ('--engine', '--platform'), positional=True)
    if values.get('help'):
        print(VERIFY_IMAGE_USAGE, end='')
        return 0
    image = values.get('positional')
    if not image:
        fail('usage: verify-oracle-image.sh [--engine docker|podman|wslc] [--platform PLATFORM] IMAGE', 2)
    native = host_platform()
    if native is None:
        fail(f'FAIL: verify-image supports only native x86_64 and aarch64 hosts, not {platform_module.machine()}', 2)
    platform = valid_platform(values.get('platform') or native)
    if platform != native:
        fail(f'every image runs at the host architecture ({native}), with no emulation; not {platform}', 2)
    engine = values.get('engine') or next((e for e in ('docker', 'podman') if shutil.which(e)), '')
    if not engine:
        fail('neither docker nor podman found')
    cid = ''
    with tempfile.TemporaryDirectory() as scratch:
        try:
            created = capture(wslc_command([engine, 'create', '--pull=never', '--platform', platform, image, '/none']),
                              check=False)
            cid = created.stdout.decode().rstrip('\r\n')  # wslc ends its lines with CRLF
            if created.status:
                raise Finished(created.status)
            oracle = copy_oracle(engine, cid, scratch)
            status = 0
            for dbm in cell.CELLS:
                for library in ('libpinyin', 'libzhuyin'):
                    path = oracle / dbm / 'lib' / (library + '.so')
                    try:
                        actual = digest_file(path)
                    except OSError as error:
                        print(f'sha256sum: {path}: {error.strerror}', file=sys.stderr)
                        raise Finished(1) from error
                    print(dbm, library, actual)
                if not prefix_ok(oracle / dbm, dbm, root, expected_platform=platform):
                    print(f'FAIL: {dbm}: stale pin/recipe or damaged recorded build', file=sys.stderr)
                    status = 1
            leaks = [path.as_posix() for path in oracle.rglob('*')
                     if path.as_posix().endswith('/lib/libpinyin/data') or path.name == 'bigram.db'
                     or path.suffix in ('.bin', '.table', '.text')]
            if leaks:
                print('FAIL: model-derived data in the image:\n' + '\n'.join(leaks), file=sys.stderr)
                status = 1
            return status
        finally:
            if cid:
                removed = subprocess.run(wslc_command([engine, 'rm', '-f', cid]), stdout=subprocess.DEVNULL,
                                         stderr=subprocess.DEVNULL, check=False)
                if removed.returncode:
                    print(f'WARNING: could not remove container {cid} ({engine} exited {removed.returncode}); '
                          'remove it by hand', file=sys.stderr)


def copy_oracle(engine, cid, scratch):
    """Copy the container's /opt/pinyin-oracle into SCRATCH; answer the copy's path.

    wslc has no cp: it exports the container's filesystem as a tar, and only the
    opt/pinyin-oracle members are extracted, under the tar module's data filter."""
    if not is_wslc(engine):
        run([engine, 'cp', f'{cid}:/opt/pinyin-oracle', f'{scratch}/oracle'], stdout=subprocess.DEVNULL)
        return Path(scratch) / 'oracle'
    archive = Path(scratch) / 'container.tar'
    run([engine, 'export', '-o', archive, cid], stdout=subprocess.DEVNULL)
    with tarfile.open(archive) as tar:
        members = [member for member in tar.getmembers()
                   if member.name.removeprefix('./').startswith('opt/pinyin-oracle/')]
        tar.extractall(Path(scratch) / 'export', members=members, filter='data')
    archive.unlink()
    return Path(scratch) / 'export/opt/pinyin-oracle'


BUILD_IMAGE_USAGE = '''Build the pinned oracle image for this host's architecture and verify it.

    python -m tools.oracle.image build-image

OXPINYIN_CAPTURE_RUNTIME names the runtime (docker by default; podman or wslc).
OXPINYIN_CAPTURE_BUILD_FLAGS carries site flags for the build, such as --network host
or --secret id=cacert,src=... behind a TLS-intercepting egress proxy (no quoting inside).
The image is tagged ghcr.io/shenghaoc/oxpinyin-oracle:TAG (TAG-arm64 on aarch64), checked
with verify-image, and its reference printed last. Nothing is pushed.
'''


def build_image(argv, *, root=ROOT, env=None):
    """Build Dockerfile.oracle on the host architecture, check it and verify it."""
    environment = os.environ if env is None else env
    if argv[:1] in (['-h'], ['--help']):
        print(BUILD_IMAGE_USAGE, end='')
        return 0
    if argv:
        fail(f'unknown option: {argv[0]}', 2)
    runtime = environment.get('OXPINYIN_CAPTURE_RUNTIME') or 'docker'
    if not shutil.which(runtime, path=environment.get('PATH')):
        fail(f'FAIL: {runtime} is not installed')
    selected = host_platform()
    if selected is None:
        fail('FAIL: the oracle image build supports only native x86_64 and aarch64', 2)
    expected = selected.split('/', 1)[1]
    image = f'{REGISTRY}:{image_tag(root)}' + ('-arm64' if expected == 'arm64' else '')
    flags = environment.get('OXPINYIN_CAPTURE_BUILD_FLAGS', '').split()
    run(wslc_command([runtime, 'build', '--platform', selected, '--build-arg', f'ORACLE_PLATFORM={selected}',
                      *flags, '-f', 'tools/oracle/Dockerfile.oracle', '-t', image, '.']), cwd=root, env=environment)
    actual = image_field(runtime, 'Architecture', image, env=environment)
    if actual != expected:
        fail(f'FAIL: built {image} for {actual}, expected {expected}')
    status = verify_image(['--engine', runtime, '--platform', selected, image], root=root)
    if status:
        return status
    print(image)
    return 0


def select_runtime(override='', *, env=None, host=None, output=print):
    """Preserve platform runtime order and an explicit override; wslc is preferred on Windows."""
    environment = os.environ if env is None else env
    host = host or ('Windows' if os.name == 'nt' else text(['uname', '-s'], env=environment))
    if host == 'Darwin':
        candidates = ['container', 'podman', 'docker']
    elif host.startswith(('Windows', 'MINGW', 'MSYS', 'CYGWIN')) or environment.get('WSL_DISTRO_NAME'):
        candidates = ['wslc', 'podman', 'docker']
    else:
        candidates = ['podman', 'docker']
    override = override or environment.get('OXPINYIN_CONTAINER', '')
    selected = override or next((name for name in candidates if shutil.which(name, path=environment.get('PATH'))), '')
    if not selected or not shutil.which(selected, path=environment.get('PATH')):
        fail(f'FAIL: no runtime available; candidates in order: {" ".join(candidates)} (override: {override or "none"})')
    done = capture([selected, '--version'], env=environment, check=False)
    if done.status:
        fail(f'FAIL: cannot query runtime version: {selected}')
    version = done.stdout.decode().rstrip(chr(10))
    if is_wslc(selected):
        # wslc --version follows its version line with copyright and privacy lines.
        version = version.splitlines()[0] if version else version
    output(f'Runtime: {selected}; {version}')
    return selected


def performance(argv, *, root=ROOT):
    """Build and enter the local performance environment, on the host architecture."""
    parser = argparse.ArgumentParser(prog='just performance')
    parser.add_argument('--engine', choices=('container', 'docker', 'podman'))
    build_mode = parser.add_mutually_exclusive_group()
    build_mode.add_argument('--build-only', action='store_true')
    build_mode.add_argument('--skip-build', action='store_true')
    parser.add_argument('--volume', help='persistent local Cargo/model cache volume')
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args(argv)
    if os.environ.get('GITHUB_ACTIONS') == 'true':
        fail('the oracle performance environment is local-only; never run it on GitHub Actions', 2)
    if platform.system() == 'Darwin':
        translated = subprocess.run(['/usr/sbin/sysctl', '-n', 'sysctl.proc_translated'],
                                    check=False, capture_output=True)
        if translated.returncode == 0 and translated.stdout.strip() == b'1':
            fail('performance measurements require native Python; this interpreter runs under Rosetta', 2)
    native = {'arm64': 'arm64', 'aarch64': 'arm64', 'x86_64': 'amd64', 'AMD64': 'amd64'}.get(platform.machine())
    if native is None:
        fail(f'unsupported native performance architecture: {platform.machine()}', 2)
    selected_platform = f'linux/{native}'
    for variable in ('DOCKER_DEFAULT_PLATFORM', 'CONTAINER_DEFAULT_PLATFORM', 'ORACLE_PLATFORM'):
        if os.environ.get(variable) not in (None, '', selected_platform):
            fail(f'{variable} forces a non-native performance platform; unset it', 2)
    engine = args.engine or select_runtime()
    inputs = hashlib.sha256()
    for name in ('.python-version', 'rust-toolchain.toml'):
        inputs.update((Path(root) / name).read_bytes())
    tag = f'oxpinyin-performance:{image_tag(root)}-{inputs.hexdigest()[:12]}-{native}'
    resources = ['--cpus', '4', '--memory', '6G'] if Path(engine).name == 'container' else []
    if not args.skip_build:
        run([engine, 'build', *resources, '--platform', selected_platform, '--build-arg', f'ORACLE_PLATFORM={selected_platform}',
             '--target', 'performance', '-f', Path(root) / 'tools/oracle/Dockerfile.oracle', '-t', tag, root])
    if args.build_only:
        return 0
    command = args.command
    if command[:1] == ['--']:
        command = command[1:]
    cache = args.volume or f'oxpinyin-performance-{selected_platform.split("/")[1]}'
    flags = ['-it'] if not command and sys.stdin.isatty() else []
    flags += resources
    common = text(['git', '-C', root, 'rev-parse', '--path-format=absolute', '--git-common-dir'])
    flags += ['-v', f'{common}:{common}:ro']
    return run([engine, 'run', '--rm', '--platform', selected_platform, *flags,
                '-v', f'{Path(root).resolve()}:/repo:ro', '-v', f'{cache}:/performance',
                '-e', 'CARGO_TARGET_DIR=/performance/target', '-e', 'TMPDIR=/performance/tmp',
                '-e', 'PYTHONDONTWRITEBYTECODE=1', '-w', '/repo', tag, *(command or ['bash'])], check=False)


def check_reproducible(argv, *, root=ROOT):
    """Build twice in distinct paths and compare only those two builds."""
    from tools.dev.measure.recipes import require_performance_environment
    require_performance_environment(os.environ)
    parser = argparse.ArgumentParser(prog='check-reproducible')
    parser.add_argument('--cells', nargs='+', choices=cell.CELLS, default=list(cell.CELLS))
    parser.add_argument('--work-dir', type=Path, required=True)
    parser.add_argument('--jobs', default='2')
    args = parser.parse_args(argv)
    child = dict(os.environ, OXPINYIN_ORACLE_RECIPE_ROOT=str(root))
    for backend in args.cells:
        builds = []
        prefix = args.work_dir / backend / 'prefix'
        directories = [args.work_dir / f'{backend}-{number}' for number in (1, 2)]
        # Refused before the first build, so the cleanup below removes only this run's paths.
        if prefix.exists():
            fail(f'reproducibility prefix already exists: {prefix}')
        for directory in directories:
            if directory.exists():
                fail(f'reproducibility work directory already exists: {directory}')
        try:
            for directory in directories:
                run(['bash', Path(root) / 'tools/oracle/build-oracle.sh', '--libs-only', '--enable-libzhuyin',
                     '--dbm', backend, '--work-dir', directory / 'build', '--prefix', prefix,
                     '--jobs', args.jobs], env=child, heavy=True)
                saved = directory / 'libraries'
                shutil.copytree(prefix / 'lib', saved, symlinks=True)
                builds.append(saved)
                shutil.rmtree(prefix)
            for library in ('libpinyin', 'libzhuyin'):
                hashes = [digest_file(directory / (library + '.so')) for directory in builds]
                print(backend, library, *hashes)
                if hashes[0] != hashes[1]:
                    fail(f'{backend} {library}: two builds differ')
        except BaseException:
            for path in (prefix, *directories):
                shutil.rmtree(path, ignore_errors=True)
            raise
    return 0


def main(argv=None):
    """Dispatch oracle image operations and the local performance environment."""
    args = list(sys.argv[1:] if argv is None else argv)

    def call():
        if args[:1] == ['check-reproducible']:
            return check_reproducible(args[1:])
        if args[:1] == ['performance']:
            return performance(args[1:])
        if args[:1] == ['image-tag']:
            print(image_tag())
            return 0
        if args[:1] == ['pin-field']:
            return pin_field(args[1:])
        if args[:1] == ['verify-image']:
            return verify_image(args[1:])
        if args[:1] == ['build-image']:
            return build_image(args[1:])
        if args[:1] == ['image-field'] and len(args) == 4:
            print(image_field(*args[1:]))
            return 0
        print('usage: python -m tools.oracle.image image-tag|pin-field|verify-image|build-image|image-field|performance ...',
              file=sys.stderr)
        return 2
    return guarded(call)


if __name__ == '__main__':
    sys.exit(main())
