"""Oracle pin, image identity and reproducibility services shared by compat callers."""
import hashlib
import os
from pathlib import Path
import re
import secrets
import shutil
import subprocess
import tempfile
import sys

from .. import pool, slot
from ..runners import Finished
from .identity import digest_file
from .process import ROOT, capture, fail, run, text


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
                 '../tkrzw/distro-probe.sh', 'oracle-pin.txt'):
        digest.update((directory / name).read_bytes())
    slug = re.sub(r'[^A-Za-z0-9_.-]', '-', base)
    return f'{commit[:12]}-{slug}-{digest.hexdigest()[:12]}'


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
    if Path(selected).name == 'container':
        print('Apple container interoperability and native-platform execution are not yet verified by the compat smoke.', file=sys.stderr)
    return selected


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
            values['image'] = option
        else:
            fail(f'unknown option: {option}', 2)
    return values


def valid_platform(value):
    """Refuse platforms without a frozen oracle record."""
    if value not in ('linux/amd64', 'linux/arm64'):
        fail(f'unsupported platform: {value} (use linux/amd64 or linux/arm64)', 2)
    return value


def verify_image(argv, *, root=ROOT):
    """Copy a scratch image's libraries, check frozen hashes and reject model data."""
    values = parse_options(argv, ('--engine', '--platform'), positional=True)
    if values.get('help'):
        from .help import TEXT
        print(TEXT['verify-image'], end='')
        return 0
    image = values.get('image')
    if not image:
        fail('usage: verify-oracle-image.sh [--engine docker|podman] [--platform PLATFORM] IMAGE', 2)
    platform = valid_platform(values.get('platform', 'linux/amd64'))
    engine = values.get('engine') or next((e for e in ('docker', 'podman') if shutil.which(e)), '')
    if not engine:
        fail('neither docker nor podman found')
    cid = ''
    with tempfile.TemporaryDirectory() as scratch:
        try:
            created = capture([engine, 'create', '--platform', platform, image, '/none'], check=False)
            cid = created.stdout.decode().rstrip('\n')
            if created.status:
                raise Finished(created.status)
            run([engine, 'cp', f'{cid}:/opt/pinyin-oracle', f'{scratch}/oracle'], stdout=subprocess.DEVNULL)
            status = 0
            for dbm in ('bdb', 'kc', 'tkrzw'):
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


BUILD_SCRIPT = '''set -eu
build-oracle.sh --libs-only --enable-libzhuyin --dbm "$1" \\
    --work-dir "$2" --prefix "$3" --jobs "$4" >/dev/null 2>/tmp/build.log ||
    { tail -40 /tmp/build.log >&2; exit 1; }
for lib in libpinyin libzhuyin; do
    printf "%s %s\\n" "$lib" "$(sha256sum "$3/lib/$lib.so" | cut -d" " -f1)"
done
'''


def reproducible(argv, *, root=ROOT):
    """Run two independent clean builds per backend through the existing bounded pool."""
    values = parse_options(argv, ('--engine', '--image', '--jobs', '--platform'))
    if values.get('help'):
        from .help import TEXT
        print(TEXT['reproducible'], end='')
        return 0
    platform = valid_platform(values.get('platform', 'linux/amd64'))
    engine = values.get('engine') or next((e for e in ('docker', 'podman', 'container') if shutil.which(e)), '')
    if not engine:
        fail('no docker, podman or container runtime found')
    try:
        default_jobs = text(['nproc'], stderr=False)
    except (OSError, Finished):
        default_jobs = '2'
    jobs = values.get('jobs', default_jobs)
    flags = ['--rm']
    if engine == 'podman':
        flags += ['--security-opt', 'label=disable']
    if engine == 'container':
        flags += ['--memory', '6G']
    build_flags = []
    proxy = os.environ.get('HTTPS_PROXY') or os.environ.get('https_proxy', '')
    ca = os.environ.get('OXPINYIN_PROXY_CA', '/root/.ccr/ca-bundle.crt')
    if engine == 'docker' and proxy and Path(ca).is_file() and Path(ca).stat().st_size:
        flags += ['--network', 'host', '-e', f'https_proxy={proxy}', '-e', f'HTTPS_PROXY={proxy}',
                  '-e', 'GIT_SSL_CAINFO=/run/ca.crt', '-e', 'SSL_CERT_FILE=/run/ca.crt',
                  '-e', 'CURL_CA_BUNDLE=/run/ca.crt', '-v', f'{ca}:/run/ca.crt:ro']
        build_flags += ['--network', 'host', '--build-arg', f'https_proxy={proxy}',
                        '--build-arg', f'HTTPS_PROXY={proxy}', '--secret', f'id=cacert,src={ca}']
    image = values.get('image', '')
    if not image:
        image = 'oxpinyin-oracle-deps:check'
        extra = [] if platform == 'linux/amd64' else ['--build-arg', f'ORACLE_PLATFORM={platform}']
        run([engine, 'build', '--platform', platform, *extra, *build_flags, '--target', 'deps',
             '-f', Path(root) / 'tools/oracle/Dockerfile.oracle', '-t', image, root], heavy=True, stdout=sys.stderr)
    status = 0
    for dbm in ('bdb', 'kc', 'tkrzw'):
        # Only this invocation's cidfile and token can authorize cleanup.
        def build(tag, work, prefix):
            def unit(context):
                from ..native import launch_status
                with tempfile.TemporaryDirectory(prefix='oracle-build.') as scratch:
                    cidfile = Path(scratch) / 'container.cid'
                    token = secrets.token_hex(16)
                    label = 'io.oxpinyin.oracle-build'
                    owned = pool.OwnedContainer((engine,), cidfile, label, token)
                    options = [] if Path(engine).name == 'container' else ['--cidfile', str(cidfile), '--label', label + '=' + token]
                    command = [engine, 'run', '--platform', platform, *flags, *options,
                               '-v', f'{root}/tools/oracle/build-oracle.sh:/usr/local/bin/build-oracle.sh:ro',
                               image, 'bash', '-c', BUILD_SCRIPT, '--', dbm, work, prefix, jobs]
                    try:
                        return context.launch(command)
                    except OSError as error:
                        return pool.Completed(launch_status(error), b'', f'{engine}: {error.strerror}\n'.encode())
                    finally:
                        # Docker and Podman report the owned ID/label even when a
                        # client was cancelled. Apple retains its original --rm path.
                        if options:
                            pool.remove_owned(owned)
            return tag, unit
        prepare = pool.admission_preparer(2, None, explicit=jobs) if re.fullmatch('[1-9][0-9]*', jobs) else None
        results = pool.run_admitted([build(dbm + '-a', '/build/a', f'/opt/oracle-a/{dbm}'),
                                     build(dbm + '-b', '/var/tmp/elsewhere/b', f'/opt/oracle-b/{dbm}')],
                                    'oracle reproducibility', cancelled=slot.CANCELLED.is_set, max_workers=2, prepare=prepare)
        from ..runners import write
        for result in results:
            write(result.stderr, error=True)
        if any(result.status for result in results):
            print(f'FAIL: {dbm}: a build exited non-zero', file=sys.stderr)
            status = 1
            continue
        if results[0].stdout != results[1].stdout:
            print(f'FAIL: {dbm} builds differ', file=sys.stderr)
            with tempfile.TemporaryDirectory() as scratch:
                paths = [Path(scratch) / (dbm + suffix + '.out') for suffix in ('-a', '-b')]
                for path, result in zip(paths, results):
                    path.write_bytes(result.stdout)
                run(['diff', *paths], stdout=sys.stderr, check=False)
            status = 1
            continue
        for line in results[0].stdout.decode().splitlines():
            library, checksum = line.split()
            print(dbm, library, checksum)
            recorded = platform_field(Path(root) / 'tools/oracle/oracle-pin.txt', platform, f'{library}_sha256_{dbm}')
            if recorded != checksum:
                print(f'FAIL: {dbm} {library} is {checksum}, tools/oracle/oracle-pin.txt records {recorded or "nothing"}', file=sys.stderr)
                status = 1
    return status
