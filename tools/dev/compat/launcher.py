"""Host compatibility orchestration, using one reserved container launch service."""
import hashlib
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time

from .. import slot
from ..native import shell_status
from ..runners import Finished
from . import boundary, identity, images, memory
from .process import ROOT, capture, fail, run, tee, text

# These helpers are verified against the same source snapshot, before execution.
STAGED = {'tools/oracle/compat-inside.sh': 'inside.sh',
          'tools/lib/bash-floor.sh': 'bash-floor.sh',
          'tools/dev/compat/identity.py': 'identity.py',
          'tools/dev/compat/guest.py': 'guest.py'}


def usage(error=False):
    """Print the preserved compat shell CLI."""
    print(Path(__file__).with_name('help.txt').read_text(), end='', file=sys.stderr if error else sys.stdout)


def arguments(argv):
    """Keep the shell's first validation pass and diagnostic/status precedence."""
    args = list(argv)
    values = dict(runtime=os.environ.get('OXPINYIN_CONTAINER', ''), platform='', cell='bdb',
                  archive='', jobs='2', memory_mib=os.environ.get('OXPINYIN_CONTAINER_MEMORY_MIB', '2560'))
    selection = []
    selected = smoke = listing = False
    while args:
        option = args.pop(0)
        if option in ('-h', '--help'):
            usage()
            raise Finished(0)
        if option in ('--memory-smoke', '--list'):
            smoke |= option == '--memory-smoke'
            listing |= option == '--list'
            continue
        if option.startswith(('--runners=', '--issue=')):
            option, value = option.split('=', 1)
            args.insert(0, value)
        if option not in ('--runtime', '--platform', '--cell', '--archive', '--jobs', '--memory-mib',
                          '--runners', '--issue', '--cases', '--timeout', '--runner-timeout'):
            print(f'unknown option: {option}', file=sys.stderr)
            usage(True)
            raise Finished(2)
        if not args:
            usage(True)
            raise Finished(2)
        value = args.pop(0)
        if option in ('--runners', '--issue'):
            if not re.sub(r'[\s,]', '', value):
                fail(f'Give a value for {option}.', 2)
            selected = True
        if option in ('--runtime', '--platform', '--cell', '--archive', '--jobs', '--memory-mib'):
            values[option[2:].replace('-', '_')] = value
        else:
            selection += [option, value]
    if values['cell'] not in ('bdb', 'kc', 'tkrzw'):
        fail('cell must be bdb, kc or tkrzw', 2)
    if not values['platform']:
        arch = text(['uname', '-m'])
        values['platform'] = {'x86_64': 'linux/amd64', 'aarch64': 'linux/arm64', 'arm64': 'linux/arm64'}.get(arch, '')
        if not values['platform']:
            fail(f'FAIL: no compatibility platform for architecture {arch}')
    if not re.fullmatch('[1-9][0-9]*', values['jobs']):
        fail('--jobs must be positive', 2)
    if not re.fullmatch('[1-9][0-9]*', values['memory_mib']):
        fail('--memory-mib must be a positive integer in MiB', 2)
    if not (selected or smoke or listing):
        fail('select --runners or --issue (use --list to discover gates)', 2)
    if smoke and selected:
        fail('FAIL: --memory-smoke cannot select runners or an issue')
    for command in ('git', 'tar'):
        if not shutil.which(command):
            fail(f'FAIL: missing {command}')
    try:
        boundary.validate(argv)
    except (OSError, ValueError) as error:
        fail(f'FAIL: {error}', 2)
    return values, selection, smoke, listing


def image_identity(root, reference, ca_identity, platform):
    """Preserve the compatibility image's original recipe byte stream."""
    digest = hashlib.sha256()
    for name in ('tools/oracle/Dockerfile.compat', 'rust-toolchain.toml', '.python-version',
                 'tools/install-python.sh', 'tools/lib/bash-floor.sh'):
        digest.update((Path(root) / name).read_bytes())
    digest.update(f'{reference}:{ca_identity}:{platform}'.encode())
    return digest.hexdigest()[:16]


def snapshot(root, directory, archive, key, reference):
    """Stage visible source and verified launch helpers, never native state or .git."""
    directory = Path(directory)
    if archive:
        if not Path(archive).is_file():
            fail(f'FAIL: local model archive missing: {archive}')
        shutil.copyfile(archive, directory / 'model.tar.gz')
    else:
        (directory / 'model.tar.gz').touch()
    paths = capture(['git', 'ls-files', '--cached', '--others', '--exclude-standard', '-z'], cwd=root).stdout
    paths = [p for p in paths.split(b'\0') if p and os.path.lexists(Path(root) / os.fsdecode(p))]
    (directory / 'files').write_bytes(b'\0'.join(paths) + (b'\0' if paths else b''))
    run(['tar', '--no-xattrs', '--null', '-T', directory / 'files', '-cf', directory / 'source.tar'],
        cwd=root, env=dict(os.environ, COPYFILE_DISABLE='1'))
    head = text(['git', 'rev-parse', 'HEAD'], cwd=root)
    status = text(['git', 'status', '--short'], cwd=root)
    source = 'Host source commit: ' + head + '\nHost source status:\n' + (status + '\n' if status else '')
    source += 'Source archive SHA-256: ' + identity.digest_file(directory / 'source.tar') + '\n'
    (directory / 'source.txt').write_text(source)
    for source, target in STAGED.items():
        shutil.copyfile(Path(root) / source, directory / target)
    digest = identity.seal_inputs(directory, key, reference, head)
    return digest, identity.verify_inputs(directory, digest)['source_sha256']


def launch(argv, *, root=ROOT):
    """Run the unchanged compat selection in isolated container state and verify exports."""
    os.chdir(root)
    os.umask(0o077)
    values, selection, smoke, listing = arguments(argv)
    if listing:
        return run([sys.executable, '-X', 'lazy_imports=normal', str(Path(root) / 'tools/bisection/run-all.py'), '--list'])
    runtime = images.select_runtime(values['runtime'])
    kind = Path(runtime).name
    host = text(['uname', '-s'])
    if host.startswith(('MINGW', 'MSYS')):
        fail('FAIL: Run this compatibility launcher from WSL Bash, not Git Bash. Native Windows development needs neither WSL nor containers.')
    if kind not in ('docker', 'podman', 'container'):
        fail('FAIL: runtime must be docker, podman, container or unverified wslc')
    if run([runtime, 'image', 'list', '--quiet'] if kind == 'container' else [runtime, 'info'],
           stdout=subprocess.DEVNULL, check=False):
        fail('FAIL: Apple container service unavailable; see container system start' if kind == 'container' else
             f'FAIL: {runtime} capability unavailable; check its installation/access (no daemon or host packages are installed by this launcher)')
    platform, jobs, size = values['platform'], values['jobs'], int(values['memory_mib'])
    reference_tag = images.image_tag(root)
    reference = 'ghcr.io/shenghaoc/oxpinyin-oracle:' + reference_tag + ('-arm64' if platform == 'linux/arm64' else '')
    deps = 'ghcr.io/shenghaoc/oxpinyin-oracle:' + reference_tag + ('-deps-arm64' if platform == 'linux/arm64' else '-deps')
    ca = os.environ.get('OXPINYIN_PROXY_CA', '')
    ca_identity = identity.digest_file(ca) if ca else 'no-custom-ca'
    key = image_identity(root, reference_tag, ca_identity, platform)
    image = 'oxpinyin-compat:' + key
    runtime = str(Path(shutil.which(runtime)).absolute())
    platform_args = ['--platform', platform]

    def present(wanted):
        if kind == 'container':
            return wanted in text([runtime, 'image', 'list', '--quiet']).splitlines()
        done = capture([runtime, 'image', 'inspect', wanted], check=False)
        return done.status == 0

    def reserved(name, options, payload, **kwargs):
        try:
            return shell_status(memory.run(runtime, kind, name, size, [*options, '--', *payload], **kwargs))
        except slot.Cancelled:
            raise
        except (OSError, ValueError) as error:
            print(f'FAIL: {error}', file=sys.stderr, flush=True)
            return 1

    if smoke:
        if not present(image):
            fail(f'FAIL: memory smoke requires existing {image}; no build attempted')
        return reserved(f'oxpinyin-memory-smoke-{os.getpid()}', [*platform_args, '--cpus', jobs],
                        [image, 'bash', '-c', memory.SMOKE, '--', str(size)], remove=True)
    run_id = time.strftime('%Y%m%dT%H%M%SZ', time.gmtime()) + f'-{os.getpid()}'
    name = f'oxpinyin-compat-{int(time.time())}-{os.getpid()}'
    with tempfile.TemporaryDirectory(prefix='oxpinyin-compat.') as temporary:
        directory = Path(temporary)
        try:
            input_identity, source_identity = snapshot(root, directory, values['archive'], key, reference_tag)
            result_root = identity.result_directory(root, source_identity, run_id)
            transport, exported = result_root / 'transport', result_root / 'result'
            transport.mkdir(parents=True)
            exported.mkdir(parents=True)
            volume = 'oxpinyin-compat-' + hashlib.sha256(str(root).encode()).hexdigest()[:16]
            relabel = ',z' if kind == 'podman' else ''
            build_flags = [] if platform == 'linux/amd64' else ['--build-arg', f'ORACLE_PLATFORM={platform}']
            run_env = ['--cpus', jobs, '-e', f'CARGO_BUILD_JOBS={jobs}', '-e', f'RUST_TEST_THREADS={jobs}']
            if kind == 'podman':
                build_flags += ['--cpu-period', '100000', '--cpu-quota', str(int(jobs) * 100000), '--jobs', '1']
            elif kind == 'container':
                build_flags += ['--cpus', jobs]
            for variable in ('HTTP_PROXY', 'HTTPS_PROXY', 'NO_PROXY', 'http_proxy', 'https_proxy', 'no_proxy'):
                value = os.environ.get(variable, '')
                if value:
                    build_flags += ['--build-arg', f'{variable}={value}']
                    run_env += ['-e', f'{variable}={value}']
            network = os.environ.get('OXPINYIN_COMPAT_NETWORK')
            if network:
                if kind == 'container':
                    fail('FAIL: Apple builder does not expose Docker/Podman --network; use a proxy reachable on its VM network')
                build_flags += ['--network', network]
                run_env += ['--network', network]
            if ca:
                build_flags += ['--secret', f'id=cacert,src={ca}']

            def build(arguments):
                code = tee([runtime, 'build', *arguments], transport / 'build.log', heavy=True, append=True)
                if code:
                    raise Finished(code)

            def pull(wanted, error):
                done = capture([runtime, *(['image', 'pull'] if kind == 'container' else ['pull']),
                                *platform_args, wanted], check=False)
                sys.stdout.buffer.write(done.stdout)
                Path(error).write_bytes(done.stderr)
                if done.status:
                    sys.stderr.buffer.write(done.stderr)
                return done.status == 0

            absent = re.compile(r'manifest unknown|name unknown|manifest.*not found|404\s+not found', re.I)
            if not present(image):
                if not present(deps):
                    error = transport / 'pull-deps.log'
                    if not pull(deps, error):
                        if not absent.search(error.read_text()):
                            fail(f'FAIL: reference image access/network/proxy failure: {deps} (retained {error}); no silent fallback')
                        print(f'Image tag absent; building {deps} from the pinned source recipe', file=sys.stderr)
                        build([*platform_args, *build_flags, '--target', 'deps', '-f', 'tools/oracle/Dockerfile.oracle', '-t', deps, '.'])
                if not present(reference):
                    error = transport / 'pull-reference.log'
                    if not pull(reference, error):
                        if not absent.search(error.read_text()):
                            fail('FAIL: reference access/network/proxy failure; see retained pull-reference.log')
                        build([*platform_args, *build_flags, '-f', 'tools/oracle/Dockerfile.oracle', '-t', reference, '.'])
                build([*platform_args, *build_flags, '--build-arg', f'DEPS_IMAGE={deps}', '--build-arg', f'REFERENCE_IMAGE={reference}',
                       '--build-arg', f'CA_IDENTITY={ca_identity}', '-f', 'tools/oracle/Dockerfile.compat', '-t', image, '.'])
            if capture([runtime, 'volume', 'inspect', volume], check=False).status:
                run([runtime, 'volume', 'create', *(['-s', '20G'] if kind == 'container' else []), volume])
            print(f'Compatibility: {platform}; native execution. No native performance claims.\nVolume: {volume}\nResults: {result_root}', flush=True)
            code = reserved(name, [*platform_args, *run_env, '-v', f'{volume}:/state',
                            '-v', f'{directory}:/input:ro{relabel}', '-w', '/'],
                            [image, 'bash', '/input/inside.sh', values['cell'], key, run_id, reference_tag,
                             input_identity, *selection, '--jobs', jobs],
                            console=transport / 'console.log', exit_confirmed=directory / 'exit-confirmed')
            if not (directory / 'exit-confirmed').is_file():
                fail('FAIL: main container exit unconfirmed; no result reader started')
            if kind == 'container':
                # The reader has its own full reservation, after main's confirmed exit.
                archive = directory / 'results.tar'
                with archive.open('wb') as stream:
                    # memory.call inherits fd 1 for start; the reader's tar is binary.
                    descriptor = os.dup(1)
                    try:
                        os.dup2(stream.fileno(), 1)
                        result = reserved(name + '-results', [*platform_args, '--cpus', jobs, '-v', f'{volume}:/state:ro', '-w', '/'],
                                          [image, 'tar', '-C', f'/state/results/container/{source_identity}/{run_id}', '-cf', '-', '.'], remove=True)
                    finally:
                        os.dup2(descriptor, 1)
                        os.close(descriptor)
                if not result:
                    result = run(['tar', '-C', exported, '-xf', archive], check=False)
            else:
                result = run([runtime, 'cp', f'{name}:/state/results/container/{source_identity}/{run_id}/.', str(exported) + '/'], check=False)
            if result:
                fail('FAIL: container result export failed; results are not verified')
            try:
                identity.verify_results(exported, input_identity)
            except (OSError, ValueError, KeyError) as error:
                print(f'FAIL: container identity: {error}', file=sys.stderr)
                fail('FAIL: container result identity verification failed')
            return code
        finally:
            # Inert/confirmed containers only: never force-remove uncertain live work.
            subprocess.run([runtime, 'rm', name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
