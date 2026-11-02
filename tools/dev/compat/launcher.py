"""Host compatibility orchestration, using one reserved container launch service."""
from concurrent.futures import ThreadPoolExecutor
import hashlib
import os
import platform
from pathlib import Path
import re
import selectors
import shutil
import subprocess
import sys
import tempfile
import time

from .. import cell, slot
from ..native import shell_status
from ..runners import Finished
from . import boundary, identity, images, memory, runtime as runtime_adapter, storage
from .process import ROOT, capture, fail, run, tee, text

# These helpers are verified against the same source snapshot, before execution.
STAGED = {'tools/oracle/compat-inside.sh': 'inside.sh',
          'tools/lib/bash-floor.sh': 'bash-floor.sh',
          'tools/dev/compat/identity.py': 'identity.py',
          'tools/dev/compat/guest.py': 'guest.py'}


def ensure_volume(runtime, kind, root=ROOT):
    """Create the checkout volume with labels or reject an ownership mismatch."""
    volume = storage.volume_name(root)
    expected = storage.labels(root)
    metadata = runtime_adapter.volume_metadata(runtime, kind, volume)
    if metadata is None:
        volume_labels = [part for key, value in expected.items() for part in ('--label', f'{key}={value}')]
        run([runtime, 'volume', 'create', *(['-s', '20G'] if kind == 'container' else []), *volume_labels, volume])
        metadata = runtime_adapter.volume_metadata(runtime, kind, volume)
    if metadata is None or metadata.get('labels') != expected:
        fail(f'FAIL: compatibility volume ownership labels do not match: {volume}')
    return volume


def storage_inspection_script():
    """Propagate failures from every volume and target size command."""
    return ('for d in /vol/*; do du -sk "$d" || exit $?; '
            'for t in "$d"/targets "$d"/container/*/targets; do '
            'if [ -d "$t" ]; then du -sk "$t" || exit $?; fi; done; '
            'n=0; for t in "$d"/targets/*/*/*/*; do '
            'if [ -d "$t" ]; then n=$((n+1)); fi; done; '
            'printf "target trees: %s %s\\n" "$n" "$d"; done')


def storage_measure(runtime, kind, names):
    """Measure named volumes in read-only batches under memory admission."""
    script = storage_inspection_script()
    for offset in range(0, len(names), 12):
        mounts = [part for volume in names[offset:offset + 12] for part in ('-v', f'{volume}:/vol/{volume}:ro')]
        name = f'oxpinyin-storage-inspect-{os.getpid()}-{offset}'
        status = shell_status(memory.run(runtime, kind, name, 512,
                              ['--cpus', '1', *mounts, '--', 'debian:trixie', 'sh', '-c', script], remove=True))
        if status:
            return status
    return 0


def checkout_owners(root=ROOT):
    """Discover current and linked checkout identities without a manual registry."""
    output = capture(['git', 'worktree', 'list', '--porcelain'], cwd=root).stdout.decode()
    paths = [line.removeprefix('worktree ') for line in output.splitlines() if line.startswith('worktree ')]
    return {storage.owner_id(Path(path).resolve()) for path in paths if Path(path).is_dir()} | {storage.owner_id(root)}


def storage_inspect(runtime, kind, *, root=ROOT):
    """List legacy names and measure only labelled project volumes."""
    command = [runtime, 'volume', 'list'] if kind == 'container' else [runtime, 'volume', 'ls', '--format', '{{.Name}}']
    names = storage.volume_names(capture(command).stdout.decode())
    owners = checkout_owners(root)
    owned = []
    for name in names:
        metadata = runtime_adapter.volume_metadata(runtime, kind, name)
        if metadata is None:
            raise ValueError(f'volume disappeared during inspection: {name}')
        state = storage.classify(name, metadata, owners)
        print(f'{name}: {state}', flush=True)
        if state.startswith(('active owner ', 'orphaned owner ')):
            owned.append(name)
    return storage_measure(runtime, kind, owned)


def maintenance_image(root=ROOT):
    """Use this checkout's existing compatibility image, which contains Python."""
    arch = {'arm64': 'linux/arm64', 'aarch64': 'linux/arm64', 'x86_64': 'linux/amd64'}.get(platform.machine())
    if not arch:
        fail('unsupported native architecture for storage maintenance', 2)
    reference = images.image_tag(root)
    ca = os.environ.get('OXPINYIN_PROXY_CA', '')
    ca_identity = identity.digest_file(ca) if ca else 'no-custom-ca'
    return 'oxpinyin-compat:' + image_identity(root, reference, ca_identity, arch)


def wait_cleanup_ready(process):
    """Drain size output while allowing cooperative cancellation."""
    pending = b''
    with selectors.DefaultSelector() as selector:
        selector.register(process.stdout, selectors.EVENT_READ)
        while True:
            slot.check_cancelled()
            if not selector.select(0.25):
                continue
            block = os.read(process.stdout.fileno(), 65536)
            if not block:
                raise ValueError('cleanup exited before confirming the volume lock')
            pending += block
            if len(pending) > 1024 * 1024:
                raise ValueError('cleanup output line exceeded its limit')
            while b'\n' in pending:
                line, pending = pending.split(b'\n', 1)
                if line == b'OXPINYIN_CLEAN_READY':
                    return
                print(line.decode(errors='replace'), flush=True)


def storage_clean(runtime, kind, categories, *, root=ROOT):
    """Clean selected paths only in this checkout's volume under the guest lock."""
    allowed = {'targets', 'work', 'results'}
    categories = set(categories)
    if not categories or not categories <= allowed:
        fail('choose at least one of --targets, --work or --results', 2)
    volume = storage.volume_name(root)
    metadata = runtime_adapter.volume_metadata(runtime, kind, volume)
    if metadata is None or metadata.get('labels') != storage.labels(root):
        fail(f'volume missing or ownership labels do not match: {volume}', 2)
    exported = identity.unredirected(Path(root) / '.oxpinyin-container/results', root)
    if 'results' in categories and exported.exists():
        if not exported.is_dir():
            fail('refusing non-directory exported results')
        run(['du', '-sk', exported])
    image = maintenance_image(root)
    if capture([runtime, 'image', 'inspect', image], check=False, stderr=False).status:
        fail('compatibility image is absent; run a compatibility check first', 2)
    name = f'oxpinyin-storage-clean-{os.getpid()}'
    options = ['--cpus', '1', '-v', f'{volume}:/state', '--',
               image, 'sh', '-c', "trap 'exit 0' TERM INT; while :; do sleep 1 & wait $!; done"]
    with ThreadPoolExecutor(max_workers=1) as executor:
        future = executor.submit(memory.run, runtime, kind, name, 512, options, remove=True)
        try:
            while True:
                slot.check_cancelled()
                if future.done():
                    future.result()  # Preserve admission or creation failures.
                    raise ValueError('maintenance container exited before cleanup')
                if not capture([runtime, 'exec', name, 'true'], check=False, stderr=False).status:
                    break
                time.sleep(0.25)
            process = subprocess.Popen([runtime, 'exec', '-i', name, 'python3', '-u', '-c', storage.CLEAN_GUEST,
                                        '/state', *sorted(categories)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                       start_new_session=True)
            try:
                wait_cleanup_ready(process)
                if kind == 'container':
                    run([runtime, 'clean', name])
                if 'results' in categories and exported.exists():
                    shutil.rmtree(exported)
                process.stdin.write(b'\n')
                process.stdin.flush()
                if process.wait(timeout=30):
                    raise ValueError('cleanup process failed')
            finally:
                if process.poll() is None:
                    slot.terminate_group(process)
                process.stdin.close()
                process.stdout.close()
        finally:
            subprocess.run([runtime, 'stop', name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                           timeout=30, check=False)
        status = shell_status(future.result())
    return status


def storage_clean_selected(runtime, kind, selections, *, root=ROOT):
    """Verify and measure every named legacy or orphan before removing any."""
    owners = checkout_owners(root)
    records = {}
    for expected, name in selections:
        if name in records:
            fail(f'duplicate volume selection: {name}', 2)
        metadata = runtime_adapter.volume_metadata(runtime, kind, name) if storage.VOLUME.fullmatch(name) else None
        state = storage.classify(name, metadata, owners) if metadata is not None else ''
        if ((expected == 'legacy' and state != 'legacy, unlabelled') or
                (expected == 'orphan' and not state.startswith('orphaned owner '))):
            fail(f'selected volume is missing or not verified {expected} storage: {name}', 2)
        records[name] = metadata
        print(f'{name}: {state}', flush=True)
    status = storage_measure(runtime, kind, list(records))
    if status:
        return status
    owners = checkout_owners(root)
    for expected, name in selections:
        metadata = runtime_adapter.volume_metadata(runtime, kind, name)
        if metadata != records[name] or (expected == 'orphan' and
                not storage.classify(name, metadata, owners).startswith('orphaned owner ')):
            fail(f'volume metadata or ownership changed before deletion: {name}', 2)
    return run([runtime, 'volume', 'rm', *records], check=False)


def consume_runtime_option(option, args, values):
    """Consume the launcher's shared runtime override for any compat command."""
    if option != '--runtime':
        return False
    if not args:
        fail('--runtime requires a value', 2)
    values['runtime'] = args.pop(0)
    return True


def storage_main(argv):
    """Dispatch an explicitly invoked storage operation."""
    args = list(argv)
    if not args or args[0] not in ('inspect', 'clean'):
        fail('usage: just compat storage inspect | clean --targets|--work|--results|--legacy NAME|--orphan NAME', 2)
    action = args.pop(0)
    values = {'runtime': os.environ.get('OXPINYIN_CONTAINER', '')}
    categories, selections = [], []
    while args:
        option = args.pop(0)
        if consume_runtime_option(option, args, values):
            continue
        if action == 'clean' and option in ('--targets', '--work', '--results'):
            categories.append(option[2:])
        elif action == 'clean' and option in ('--legacy', '--orphan') and args:
            selections.append((option[2:], args.pop(0)))
        else:
            fail(f'invalid storage option: {option}', 2)
    if action == 'clean' and (bool(categories) == bool(selections)):
        fail('select cleanup categories or explicitly named legacy/orphan volumes', 2)
    runtime = runtime_adapter.select_runtime(values['runtime'])
    kind = Path(runtime).name
    if kind not in ('container', 'podman', 'docker'):
        fail('storage maintenance requires Apple container, Podman or Docker', 2)
    executable = shutil.which(runtime)
    if not executable:
        fail('container runtime unavailable')
    if action == 'inspect':
        return storage_inspect(executable, kind)
    if selections:
        return storage_clean_selected(executable, kind, selections)
    return storage_clean(executable, kind, categories)


def usage(error=False):
    """Print the preserved compat shell CLI."""
    print(Path(__file__).with_name('help.txt').read_text(), end='', file=sys.stderr if error else sys.stdout)


def arguments(argv):
    """Keep the shell's first validation pass and diagnostic/status precedence."""
    args = list(argv)
    values = dict(runtime=os.environ.get('OXPINYIN_CONTAINER', ''), platform='', cell=cell.default_cell(),
                  archive='', jobs='2', memory_mib=os.environ.get('OXPINYIN_CONTAINER_MEMORY_MIB', '2560'))
    selection = []
    selected = smoke = listing = False
    while args:
        option = args.pop(0)
        if option in ('-h', '--help'):
            usage()
            raise Finished(0)
        if consume_runtime_option(option, args, values):
            continue
        if option in ('--memory-smoke', '--list'):
            smoke |= option == '--memory-smoke'
            listing |= option == '--list'
            continue
        if option.startswith(('--runners=', '--issue=', '--cases-prefix=')):
            option, value = option.split('=', 1)
            args.insert(0, value)
        if option not in ('--runtime', '--platform', '--cell', '--archive', '--jobs', '--memory-mib',
                          '--runners', '--issue', '--cases', '--cases-prefix', '--timeout', '--runner-timeout'):
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
    if values['cell'] not in cell.CELLS:
        fail('cell must be bdb, kc or tkrzw', 2)
    arch = text(['uname', '-m'])
    native = {'x86_64': 'linux/amd64', 'aarch64': 'linux/arm64', 'arm64': 'linux/arm64'}.get(arch, '')
    if not native:
        fail(f'FAIL: no compatibility platform for architecture {arch}')
    if values['platform'] and values['platform'] != native:
        fail('FAIL: compatibility requires the native architecture; emulation is unsupported', 2)
    values['platform'] = native
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
    runtime = runtime_adapter.select_runtime(values['runtime'])
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
        done = capture([runtime, 'image', 'inspect', wanted], check=False, stderr=False)
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
            volume = storage.volume_name(root)
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
                                *platform_args, wanted], check=False, stderr=False)
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
            content = runtime_adapter.content_digest(runtime, kind, image)
            triple = {'linux/arm64': 'aarch64-unknown-linux-gnu',
                      'linux/amd64': 'x86_64-unknown-linux-gnu'}[platform]
            initial = identity.verify_inputs(directory, input_identity)
            input_identity = identity.seal_inputs(directory, key, reference_tag, initial['head'],
                                                  content, triple, values['cell'])
            image_check = lambda record: runtime_adapter.verify_created_image(record, content)
            ensure_volume(runtime, kind, root)
            print(f'Compatibility: {platform}; native execution. No native performance claims.\nVolume: {volume}\nResults: {result_root}', flush=True)
            code = reserved(name, [*platform_args, *run_env, '-v', f'{volume}:/state',
                            '-v', f'{directory}:/input:ro{relabel}', '-w', '/'],
                            [image, 'bash', '/input/inside.sh', values['cell'], key, run_id, reference_tag,
                             input_identity, *selection, '--jobs', jobs],
                            console=transport / 'console.log', exit_confirmed=directory / 'exit-confirmed',
                            image_check=image_check)
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
                                          [image, 'tar', '-C', f'/state/results/container/{source_identity}/{run_id}', '-cf', '-', '.'], remove=True, image_check=image_check)
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
