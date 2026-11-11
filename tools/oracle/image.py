"""Oracle pin fields, image tag, image verification and runtime selection.

Standard library plus the shared tools.dev process helpers. Used by
`just capture-pin` and cloud provisioning:

    python -m tools.oracle.image image-tag
    python -m tools.oracle.image pin-field [--platform PLATFORM] KEY
    python -m tools.oracle.image verify-image [--engine container|docker|podman|wslc] [--platform PLATFORM] IMAGE
    python -m tools.oracle.image build-image
    python -m tools.oracle.image image-field ENGINE FIELD IMAGE
    python -m tools.oracle.image capture-pin RUN
    python -m tools.oracle.image run-in-capture-image OUT COMMAND [ARGUMENTS...]
    python -m tools.oracle.image provision-oracle [--cells CELLS] [--jobs N]

Every runtime command is written in docker's grammar and passed through runtime_command.
docker and podman take it as written. wslc (WSL containers, Windows) has no --platform,
no top-level cp and no rm, and its inspect prints only JSON. Apple's container (macOS)
groups pull and tag under image, names rm delete and info `system status`, and prints
JSON. Neither takes the z mount option. wslc_command, apple_command and the JSON branches
below translate those calls (docs/runbooks/setup.md).
"""
import json
import argparse
import hashlib
import io
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

from tools import cachedir
from tools.dev import cell, model
from tools.dev.process import ROOT, capture, fail, run, text
from tools.dev.runners import Finished, guarded
from tools.oracle.build import fields, recipe_hash

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


def is_apple(engine):
    """True for Apple's container CLI, by any path: a container on macOS only."""
    return Path(str(engine)).name == 'container' and host_system() == 'Darwin'


def runtime_command(command):
    """Translate one docker-grammar command for its runtime; docker and podman take it as written.

    wslc and Apple's container reject the SELinux relabel option z, so a mount's ,z or :z is
    dropped for both. Then wslc_command or apple_command translates the rest."""
    engine = command[0]
    if not (is_wslc(engine) or is_apple(engine)):
        return list(command)
    translated, rest = [engine], list(command[1:])
    while rest:
        part = rest.pop(0)
        translated.append(part)
        if part in ('-v', '--volume') and rest:
            spec = str(rest.pop(0))
            for suffix, kept in ((':ro,z', ':ro'), (':z,ro', ':ro'), (',z', ''), (':z', '')):
                if spec.endswith(suffix):
                    spec = spec[:-len(suffix)] + kept
                    break
            translated.append(spec)
    return wslc_command(translated) if is_wslc(engine) else apple_command(translated)


APPLE_GROUPED = {'pull': ['image', 'pull'], 'tag': ['image', 'tag'], 'rm': ['delete'], 'info': ['system', 'status']}


def network_option(runtime, value, *, build):
    """Whether RUNTIME (wslc or Apple's container) keeps `--network VALUE`.

    Neither has host networking: --network host is dropped with a one-line notice that the
    runtime's default network is used. Neither build takes --network at all, so any other value
    there is refused; a run or create keeps it (the tools' own --network none among them)."""
    name = Path(str(runtime)).name
    if value == 'host':
        print(f'{name}: no --network host; the runtime\'s default network is used', file=sys.stderr, flush=True)
        return False
    if build:
        fail(f'FAIL: {name} build has no --network; it cannot honour --network {value}', 2)
    return True


def apple_command(command):
    """Translate one docker-grammar command for Apple's container (docs/runbooks/setup.md, macOS).

    Its pull and tag live under `image`, rm is delete, and info is `system status`. The container
    group word docker accepts (`container inspect`) is dropped, since the CLI is itself named
    container. --network goes through network_option. -v documents no :ro, so a
    read-only bind is written as --mount ...,readonly. The pull policy flags of create are dropped:
    Apple's create has none."""
    translated, rest = [command[0]], list(command[1:])
    if rest[:1] == ['container']:
        rest.pop(0)
    if rest[:1] and str(rest[0]) in APPLE_GROUPED:
        rest[:1] = APPLE_GROUPED[str(rest[0])]
    pull = rest[:2] == ['image', 'pull']
    build = rest[:1] == ['build']
    while rest:
        part = rest.pop(0)
        if pull and part == '--quiet':
            continue
        if part == '--network' and rest:
            value = rest.pop(0)
            if network_option(command[0], str(value), build=build):
                translated += [part, value]
        elif str(part).startswith('--network='):
            if network_option(command[0], str(part).split('=', 1)[1], build=build):
                translated.append(part)
        elif command[1:2] == ['create'] and str(part).startswith('--pull'):
            continue
        elif part in ('-v', '--volume') and rest and str(rest[0]).endswith(':ro'):
            source, _, target = str(rest.pop(0))[:-len(':ro')].rpartition(':')
            translated += ['--mount', f'type=bind,source={source},target={target},readonly']
        else:
            translated.append(part)
    return translated


def wslc_command(command):
    """Translate one docker-grammar command for wslc; other engines pass through unchanged.

    wslc runs only the host architecture and has no --platform, so the flag and its value are
    dropped (callers check the host architecture first). rm is remove there, also under the
    container group. --network goes through network_option: no host networking, and no --network
    on its build."""
    if not is_wslc(command[0]):
        return list(command)
    translated, rest = [command[0]], list(command[1:])
    if rest[:1] == ['rm']:
        rest[0] = 'remove'
    elif rest[:2] == ['container', 'rm']:
        rest[1] = 'remove'
    build = rest[:1] == ['build']
    while rest:
        part = rest.pop(0)
        if part == '--platform':
            rest.pop(0)
        elif part == '--network' and rest:
            value = rest.pop(0)
            if network_option(command[0], str(value), build=build):
                translated += [part, value]
        elif str(part).startswith('--network='):
            if network_option(command[0], str(part).split('=', 1)[1], build=build):
                translated.append(part)
        elif not str(part).startswith('--platform='):
            translated.append(part)
    return translated


def host_platform():
    """The linux/ platform of this host's architecture, or None for another architecture."""
    architecture = ARCHITECTURES.get(platform.machine())
    return architecture and 'linux/' + architecture


def image_field(engine, field, image, *, env=None, platform=None):
    """One top-level field of an image's inspect record: Go template, or wslc's or Apple's JSON.

    Apple's record is an OCI index: Id is the index digest, and Architecture is the linux
    variant's platform architecture."""
    if is_wslc(engine):
        done = capture([engine, 'image', 'inspect', '--format', 'json', image], env=env)
        return str(json.loads(done.stdout.decode())[0][field])
    if is_apple(engine):
        record = json.loads(capture([engine, 'image', 'inspect', image], env=env).stdout.decode())[0]
        if field == 'Id':
            return str(record['configuration']['descriptor']['digest'])
        if field == 'Architecture':
            # The variant of the requested platform (the host's by default), not the first linux one.
            wanted = (platform or host_platform() or '').partition('/')[2]
            found = [str(variant['platform']['architecture']) for variant in record['variants']
                     if variant['platform'].get('os') == 'linux']
            return wanted if wanted in found else ','.join(found)
        raise ValueError(f'no {field} in an Apple image record')
    return text([engine, 'image', 'inspect', '--format', '{{.' + field + '}}', image], env=env)


def container_owner(engine, container, label, *, run=None):
    """A container's ID and its LABEL value, as `ID VALUE`, or None when inspect fails.

    docker and podman print a Go template; wslc prints docker-shaped JSON and Apple's container
    its own (configuration.id, configuration.labels). RUN takes the argv and answers a
    CompletedProcess with text output."""
    run = run or (lambda argv: subprocess.run(list(argv), stdin=subprocess.DEVNULL, capture_output=True,
                                              text=True, check=False, timeout=60))
    if not (is_wslc(engine[0]) or is_apple(engine[0])):
        template = '{{.Id}} {{index .Config.Labels "%s"}}' % label
        found = run([*engine, 'container', 'inspect', '--format', template, container])
        return None if found.returncode else found.stdout.strip()
    found = run(runtime_command([*engine, 'container', 'inspect', container]))
    if found.returncode:
        return None
    try:
        record = json.loads(found.stdout)[0]
        if is_apple(engine[0]):
            record = record['configuration']
            return f"{record['id']} {(record.get('labels') or {}).get(label, '')}"
        return f"{record['Id']} {(record['Config'].get('Labels') or {}).get(label, '')}"
    except (ValueError, LookupError, TypeError):
        return None


def container_ids(engine, label, value, *, run=None):
    """The IDs of the containers that carry LABEL=VALUE, or [] when the runtime cannot list them.

    docker, podman and wslc filter the listing; Apple's container lists JSON and the labels are
    read here. The listing only names candidates: the caller checks each with container_owner.
    RUN is as there."""
    run = run or (lambda argv: subprocess.run(list(argv), stdin=subprocess.DEVNULL, capture_output=True,
                                              text=True, check=False, timeout=60))
    if is_apple(engine[0]):
        found = run(runtime_command([*engine, 'container', 'ls', '-a', '--format', 'json']))
        if found.returncode:
            return []
        try:
            records = [item['configuration'] for item in json.loads(found.stdout)]
            return [record['id'] for record in records if (record.get('labels') or {}).get(label) == value]
        except (ValueError, LookupError, TypeError):
            return []
    found = run(runtime_command([*engine, 'container', 'ls', '-a', '-q', '--no-trunc',
                                 '--filter', f'label={label}={value}']))
    return [] if found.returncode else found.stdout.split()


def digest_file(path):
    """Return the hexadecimal SHA-256 of file bytes; read errors propagate."""
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def platform_field(pin, platform, key):
    """Read a platform input field; unsupported platforms have no value."""
    suffix = {'linux/amd64': '', 'linux/arm64': '_linux_arm64'}.get(platform)
    return '' if suffix is None else fields(pin).get(key + suffix, '')


def image_tag(root=ROOT):
    """The oracle image tag: the pinned commit, the base image and the recipe hash."""
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
        fail('usage: verify-image [--engine container|docker|podman|wslc] [--platform PLATFORM] IMAGE', 2)
    native = host_platform()
    if native is None:
        fail(f'FAIL: verify-image supports only native x86_64 and aarch64 hosts, not {platform_module.machine()}', 2)
    platform = valid_platform(values.get('platform') or native)
    if platform != native:
        fail(f'every image runs at the host architecture ({native}), with no emulation; not {platform}', 2)
    engine = values.get('engine') or select_runtime(output=lambda line: print(line, file=sys.stderr))
    cid = ''
    with tempfile.TemporaryDirectory() as scratch:
        try:
            if is_apple(engine):
                # Apple's container can neither cp nor export a container that never started, and
                # this FROM scratch image cannot start: the image itself is read, and no container made.
                oracle = saved_oracle(engine, image, platform, scratch)
            else:
                created = capture(runtime_command([engine, 'create', '--pull=never', '--platform', platform, image, '/none']),
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
                removed = subprocess.run(runtime_command([engine, 'rm', '-f', cid]), stdout=subprocess.DEVNULL,
                                         stderr=subprocess.DEVNULL, check=False)
                if removed.returncode:
                    print(f'WARNING: could not remove container {cid} ({engine} exited {removed.returncode}); '
                          'remove it by hand', file=sys.stderr)


def copy_oracle(engine, cid, scratch):
    """Copy the container's /opt/pinyin-oracle into SCRATCH; answer the copy's path.

    docker and podman copy with cp. wslc has no top-level cp: it exports the container's
    filesystem as a tar, and extract_oracle takes its opt/pinyin-oracle members. Apple's
    container makes no container at all; saved_oracle reads its saved image instead."""
    if not is_wslc(engine):
        run([engine, 'cp', f'{cid}:/opt/pinyin-oracle', f'{scratch}/oracle'], stdout=subprocess.DEVNULL)
        return Path(scratch) / 'oracle'
    archive = Path(scratch) / 'container.tar'
    run([engine, 'export', '-o', archive, cid], stdout=subprocess.DEVNULL)
    with tarfile.open(archive) as tar:
        extract_oracle(tar, Path(scratch) / 'export')
    archive.unlink()
    return Path(scratch) / 'export/opt/pinyin-oracle'


ORACLE_MEMBERS = 'opt/pinyin-oracle/'


def oracle_members(tar):
    """TAR's members under opt/pinyin-oracle/, with or without a leading ./."""
    return [member for member in tar.getmembers() if member.name.removeprefix('./').startswith(ORACLE_MEMBERS)]


def extract_oracle(tar, destination):
    """Extract TAR's opt/pinyin-oracle members into DESTINATION under the tar module's data filter.

    A file already at a member's path (from an earlier image layer) is replaced."""
    members = oracle_members(tar)
    for member in members:
        target = Path(destination) / member.name.removeprefix('./')
        if not member.isdir() and (target.is_symlink() or target.is_file()):
            target.unlink()
    tar.extractall(destination, members=members, filter='data')


OCI_INDEXES = ('application/vnd.oci.image.index.v1+json', 'application/vnd.docker.distribution.manifest.list.v2+json')
OCI_MANIFEST = 'application/vnd.oci.image.manifest.v1+json'
LAYER_MODES = (('layer.v1.tar', 'r:'), ('layer.v1.tar+gzip', 'r:gz'))


def saved_oracle(engine, image, platform, scratch):
    """Read /opt/pinyin-oracle out of IMAGE without a container: `image save`, then its OCI layout.

    The saved archive holds oci-layout, index.json and blobs/sha256/<hex>. Indexes are followed,
    by the entry whose platform is PLATFORM where there are several, to the image manifest. Every
    blob read is checked against its digest. The layers (tar or tar+gzip) are applied in manifest
    order, so a later layer's file replaces an earlier one; only their opt/pinyin-oracle members are
    extracted, and a whiteout among them fails. Answer the oracle directory, as copy_oracle does."""
    archive = Path(scratch) / 'image.tar'
    run(runtime_command([engine, 'image', 'save', '-o', archive, '--platform', platform, image]), stdout=subprocess.DEVNULL)
    destination = Path(scratch) / 'saved'
    with tarfile.open(archive) as layout:
        files = {member.name.removeprefix('./'): member for member in layout.getmembers() if member.isfile()}

        def read(name):
            if name not in files:
                fail(f'FAIL: {image}: the saved image has no {name}')
            return layout.extractfile(files[name]).read()

        def blob(descriptor):
            algorithm, _, digest = str(descriptor.get('digest', '')).partition(':')
            if algorithm != 'sha256' or not re.fullmatch(r'[0-9a-f]{64}', digest):
                fail(f"FAIL: {image}: unsupported blob digest {descriptor.get('digest')!r}")
            data = read(f'blobs/sha256/{digest}')
            if hashlib.sha256(data).hexdigest() != digest:
                fail(f'FAIL: {image}: blob sha256:{digest} does not match its digest')
            return data

        if 'oci-layout' not in files:
            fail(f'FAIL: {image}: the saved image is not an OCI image layout (no oci-layout)')
        manifest = oci_manifest(json.loads(read('index.json')), blob, platform, image)
        for layer in manifest.get('layers', []):
            media = str(layer.get('mediaType', ''))
            mode = next((mode for suffix, mode in LAYER_MODES if media.endswith(suffix)), None)
            if mode is None:
                fail(f'FAIL: {image}: layer {layer.get("digest")} has media type {media!r}; '
                     'only layer.v1.tar and layer.v1.tar+gzip layers are read')
            with tarfile.open(fileobj=io.BytesIO(blob(layer)), mode=mode) as tar:
                whiteouts = [member.name for member in oracle_members(tar) if Path(member.name).name.startswith('.wh.')]
                if whiteouts:
                    fail(f'FAIL: {image}: layer {layer["digest"]} whites out {whiteouts[0]}; '
                         'whiteouts under opt/pinyin-oracle are not applied')
                extract_oracle(tar, destination)
    archive.unlink()
    oracle = destination / 'opt/pinyin-oracle'
    if not oracle.is_dir():
        fail(f'FAIL: {image}: no /opt/pinyin-oracle in its layers')
    return oracle


def oci_manifest(index, blob, platform, image):
    """Follow INDEX's entries, by PLATFORM where there are several, to the image manifest."""
    system, _, architecture = platform.partition('/')

    def matches(descriptor):
        found = descriptor.get('platform') or {}
        return found.get('os') == system and found.get('architecture') == architecture
    for _ in range(8):
        entries = index.get('manifests') or []
        if len(entries) == 1 and not entries[0].get('platform'):
            chosen = entries[0]
        else:
            chosen = next((entry for entry in entries if matches(entry)), None)
            if chosen is None:
                found = ', '.join(f"{(entry.get('platform') or {}).get('os')}/{(entry.get('platform') or {}).get('architecture')}"
                                  for entry in entries)
                fail(f'FAIL: {image}: no {platform} entry in the saved image index ({found or "no entries"})')
        record = json.loads(blob(chosen))
        media = chosen.get('mediaType') or record.get('mediaType')
        if media in OCI_INDEXES:
            index = record
        elif media == OCI_MANIFEST:
            return record
        else:
            fail(f'FAIL: {image}: index entry {chosen.get("digest")} has media type {media!r}, not an index or an OCI image manifest')
    fail(f'FAIL: {image}: the saved image indexes nest more than 8 deep')


BUILD_IMAGE_USAGE = '''Build the pinned oracle image for this host's architecture and verify it.

    python -m tools.oracle.image build-image

The runtime is select_runtime's: OXPINYIN_CAPTURE_RUNTIME, else OXPINYIN_CONTAINER, else the
platform's first installed runtime (wslc, Podman, Docker on Windows; Apple container, Podman,
Docker on macOS; Podman, Docker on Linux); its `Runtime:` line is printed first.
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
    selected = host_platform()
    if selected is None:
        fail('FAIL: the oracle image build supports only native x86_64 and aarch64', 2)
    runtime = select_runtime(override=environment.get('OXPINYIN_CAPTURE_RUNTIME', ''), env=environment)
    expected = selected.split('/', 1)[1]
    image = f'{REGISTRY}:{image_tag(root)}' + ('-arm64' if expected == 'arm64' else '')
    flags = environment.get('OXPINYIN_CAPTURE_BUILD_FLAGS', '').split()
    run(runtime_command([runtime, 'build', *performance_resources(runtime), '--platform', selected, '--build-arg', f'ORACLE_PLATFORM={selected}',
                      *flags, '-f', 'tools/oracle/Dockerfile.oracle', '-t', image, '.']), cwd=root, env=environment)
    actual = image_field(runtime, 'Architecture', image, env=environment, platform=selected)
    if actual != expected:
        fail(f'FAIL: built {image} for {actual}, expected {expected}')
    status = verify_image(['--engine', runtime, '--platform', selected, image], root=root)
    if status:
        return status
    print(image)
    return 0


CAPTURE_RECIPE = ('tools/oracle/Dockerfile.capture', 'tools/oracle/versions.py',
                  'tools/oracle/version-manifest.json', '.python-version')
MULTIARCH = {'amd64': 'x86_64-linux-gnu', 'arm64': 'aarch64-linux-gnu'}
RUN_NAME = re.compile(r'[A-Za-z0-9_-]+')


def capture_recipe(root=ROOT):
    """The capture image's recipe hash: sha256sum's text lines of CAPTURE_RECIPE, hashed again."""
    lines = ''.join(f'{digest_file(Path(root) / name)}  {name}\n' for name in CAPTURE_RECIPE)
    return hashlib.sha256(lines.encode()).hexdigest()[:12]


def run_user():
    """docker's --user for the invoking user; podman maps it already, and Windows has no uid."""
    if not hasattr(os, 'getuid'):
        return []
    return ['--user', f'{os.getuid()}:{os.getgid()}']


def capture_image_command(runtime, out, command, *, root, data, image, selected, reference=None, provision='', model=''):
    """The run command for COMMAND in the capture image IMAGE (docker grammar, then wslc_command).

    PROVISION, a `provision oracle` cache, is mounted read-only at /provision and MODEL, a model20
    cache, at /model20, each only when given."""
    architecture = selected.split('/', 1)[1]
    mounts = [*(['-v', f'{Path(provision)}:/provision:ro'] if provision else []),
              *(['-v', f'{Path(model)}:/model20:ro'] if model else [])]
    user = [] if Path(runtime).name.startswith('podman') else run_user()
    python = 'python' + re.match(r'[0-9]+\.[0-9]+', (Path(root) / '.python-version').read_text()).group()
    return runtime_command([
        runtime, 'run', '--rm', '--platform', selected, '--network', 'none', *user,
        '-e', 'HOME=/tmp', '-e', 'TMPDIR=/tmp', '-e', 'PYTHONPYCACHEPREFIX=/tmp/pyc',
        '-e', f'OXPINYIN_CAPTURE_IMAGE={image}',
        '-e', f'OXPINYIN_TKRZW_LIBRARY=/usr/lib/{MULTIARCH[architecture]}/libtkrzw.so.1',
        '-v', f'{Path(root)}:/work:ro', '-v', f'{Path(root) / out}:/work/{out}:rw', '-v', f'{data}:/data:ro',
        *mounts, '-w', '/work', reference or image, python, '-X', 'lazy_imports=normal', *command])


def capture_build_command(runtime, reference, capture_image, *, selected, flags):
    """The capture image build: Dockerfile.capture on top of REFERENCE."""
    if Path(runtime).name.startswith('podman'):
        pull = ['--pull=never']
    elif is_wslc(runtime) or is_apple(runtime):
        pull = []  # wslc's and Apple container's build --pull is an on/off switch that takes no value
    else:
        pull = ['--pull=false']
    return runtime_command([runtime, 'build', *performance_resources(runtime), '--platform', selected, *pull, *flags, '-f', 'tools/oracle/Dockerfile.capture',
                         '--build-arg', f'REFERENCE_IMAGE={reference}', '-t', capture_image, '.'])


def quiet(command, env):
    """A command's status with its output discarded."""
    return run(command, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)


def run_in_capture_image(out, command, *, root=ROOT, env=None):
    """Run COMMAND, with the image's managed Python, in the capture image the pin runs in.

    Run on a native x86_64 or aarch64 host. The published oracle image is pulled when absent;
    the capture image (Dockerfile.capture) is built on top of it. The checkout is mounted
    read-only at /work, OUT (a path relative to the checkout) writable at /work/OUT, and the
    pinned libpinyin-data (`just fetch-contract-data`) at /data. The container has no network.
    With OXPINYIN_CAPTURE_PROVISION set to a directory, it is mounted read-only at /provision too,
    and OXPINYIN_CAPTURE_MODEL, a model20 cache, at /model20.
    The runtime is select_runtime's (OXPINYIN_CAPTURE_RUNTIME, else OXPINYIN_CONTAINER, else the
    platform's default), and its `Runtime:` line is printed; OXPINYIN_CAPTURE_BUILD_FLAGS
    carries site flags for the image build (no quoting inside)."""
    environment = os.environ if env is None else env
    selected = host_platform()
    if selected is None:
        fail('FAIL: capture supports only native x86_64 and aarch64', 2)
    runtime = select_runtime(override=environment.get('OXPINYIN_CAPTURE_RUNTIME', ''), env=environment)
    architecture = selected.split('/', 1)[1]
    suffix = '-arm64' if architecture == 'arm64' else ''
    tag = image_tag(root)
    reference = f'{REGISTRY}:{tag}{suffix}'
    capture_image = f'oxpinyin-capture:{tag}{suffix}-{capture_recipe(root)}'
    if quiet([runtime, 'image', 'inspect', reference], environment):
        if run(runtime_command([runtime, 'pull', '--platform', selected, reference]), env=environment, check=False):
            fail(f'FAIL: {reference} is unavailable; build it with: '
                 'uv run --no-project --managed-python python -X lazy_imports=normal -m tools.oracle.image build-image')
    actual = image_field(runtime, 'Architecture', reference, env=environment, platform=selected)
    if actual != architecture:
        fail(f'FAIL: {reference} is {actual}, expected native {architecture}')
    if quiet([runtime, 'image', 'inspect', capture_image], environment):
        flags = environment.get('OXPINYIN_CAPTURE_BUILD_FLAGS', '').split()
        if run(capture_build_command(runtime, reference, capture_image, selected=selected, flags=flags),
               cwd=root, env=environment, check=False):
            raise Finished(1)
    actual = image_field(runtime, 'Architecture', capture_image, env=environment, platform=selected)
    if actual != architecture:
        fail(f'FAIL: {capture_image} is {actual}, expected native {architecture}')
    fetched = capture([sys.executable, '-X', 'lazy_imports=normal', '-m', 'tools.dev', 'fetch-contract-data'],
                      cwd=root, env=environment)
    data = fetched.stdout.decode().splitlines()[-1].strip()
    image = image_field(runtime, 'Id', capture_image, env=environment)
    (Path(root) / out).mkdir(parents=True, exist_ok=True)
    status = run(capture_image_command(runtime, out, command, root=root, data=data, image=image, selected=selected,
                                       reference=capture_image if is_apple(runtime) else None,
                                       provision=environment.get('OXPINYIN_CAPTURE_PROVISION', ''),
                                       model=environment.get('OXPINYIN_CAPTURE_MODEL', '')),
                 cwd=root, env=environment, check=False)
    if is_apple(runtime) and image_field(runtime, 'Id', capture_image, env=environment) != image:
        fail(f'FAIL: capture image tag {capture_image} changed during the run')
    return status


CAPTURE_PIN_USAGE = 'usage: capture-pin RUN'


def capture_pin(argv, *, root=ROOT, env=None):
    """Capture one run of the proposed pin observations (docs/runbooks/pin-captures.md).

    RUN names the run (1, 2, ...). The C API observations run on the image's tkrzw cell on the
    pinned libpinyin-data. The utility runs use the bdb, kc and tkrzw builds of `python -m tools.dev
    provision oracle` in the provision_cache directory CACHE: CACHE/oracle/CELL and the build
    tree beside it, mounted read-only at /provision. The data sequence reads the verified model20
    extraction MODEL/extracted, the model20 entry of the cache root that `provision oracle`
    fetched into, mounted read-only at /model20. tools/oracle/pin_capture.py checks the prefixes,
    the loaded libraries, the utility builds and the data, then stages the proposed observations
    and capture.json in target/pin-observations/RUN/. Nothing else is written."""
    environment = os.environ if env is None else env
    if argv[:1] in (['-h'], ['--help']):
        print(CAPTURE_PIN_USAGE)
        return 0
    if len(argv) != 1:
        fail(CAPTURE_PIN_USAGE, 2)
    name = argv[0]
    if not RUN_NAME.fullmatch(name):
        fail(f'FAIL: a run name uses letters, digits, _ and -: {name}', 2)
    provision = provision_cache(environment, root)
    if not provision.is_dir():
        fail(f'FAIL: {provision} is not a directory; run `just provision-oracle` with this OXPINYIN_CACHE', 2)
    model = model_cache(environment)
    if not (Path(model) / 'extracted').is_dir():
        fail(f'FAIL: {Path(model) / "extracted"} is not a model20 extraction; '
             'run `just provision-oracle` with this OXPINYIN_CACHE', 2)
    child = dict(environment, OXPINYIN_CAPTURE_PROVISION=str(Path(provision).resolve()),
                 OXPINYIN_CAPTURE_MODEL=str(Path(model).resolve()))
    return run_in_capture_image('target/pin-observations', [
        'tools/oracle/pin_capture.py', 'capture', name, '--prefix', '/opt/reference/tkrzw', '--data', '/data',
        '--provision-cache', '/provision', '--model-dir', '/model20/extracted',
        '--staging', '/work/target/pin-observations'], root=root, env=child)


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


def performance_tag(root, native):
    """The performance image's tag: the oracle tag, the interpreter and toolchain pins, the architecture."""
    inputs = hashlib.sha256()
    for name in ('.python-version', 'rust-toolchain.toml'):
        inputs.update((Path(root) / name).read_bytes())
    return f'oxpinyin-performance:{image_tag(root)}-{inputs.hexdigest()[:12]}-{native}'


def performance_resources(engine):
    """Apple's container VM gets 4 CPUs and 6 GB; the other runtimes take the host's."""
    return ['--cpus', '4', '--memory', '6G'] if is_apple(engine) else []


def performance_build_command(engine, root, selected_platform, tag):
    """The build of Dockerfile.oracle's performance target (docker grammar)."""
    return [engine, 'build', *performance_resources(engine), '--platform', selected_platform,
            '--build-arg', f'ORACLE_PLATFORM={selected_platform}', '--target', 'performance',
            '-f', Path(root) / 'tools/oracle/Dockerfile.oracle', '-t', tag, root]


def provision_cache(env, root=ROOT):
    """The provisioning cache `provision` and `provision-oracle` share:
    pin-<the first 12 digits of the libpinyin commit>/ under the cache root (tools/cachedir.py)."""
    commit = fields(Path(root) / 'tools/oracle/oracle-pin.txt')['libpinyin_commit']
    return cachedir.root(env) / f'pin-{commit[:12]}'


def model_cache(env):
    """The model20 entry of the cache root, which every provisioning run shares."""
    return model.default_cache(env)


PROVISION_ORACLE_USAGE = '''usage: just provision-oracle [--cells CELLS] [--jobs N]

Run `provision oracle` in the performance image, through the selected runtime
(OXPINYIN_CONTAINER overrides it). The cache root OXPINYIN_CACHE is mounted at
/cache, and its provisioning cache pin-<commit> also at /provision, where it
receives oracle/<cell> (the pin's libraries and data). The image is built when absent;
OXPINYIN_CAPTURE_BUILD_FLAGS carries site flags for that build (no quoting inside).
'''


def provision_oracle_command(engine, root, cache, tag, selected_platform, argv, shared):
    """The run of `provision oracle` in the performance image TAG (docker grammar).

    The host's cache root SHARED is mounted at /cache, which the run receives as OXPINYIN_CACHE,
    so the model20 entry is the host's. The provisioning cache CACHE is mounted at /provision
    and named with --cache-dir, so a prefix keeps its path /provision/oracle/<cell>.
    On rootful docker the run is the invoking user (run_user), so the cache it writes stays theirs;
    podman maps the user already, as for the capture run, and Apple's container keeps the run its
    macOS lane verified. uv then needs a writable HOME and cache, in /tmp; the managed interpreter
    under /opt/uv/python is world-readable (Dockerfile.oracle)."""
    user = [] if Path(engine).name.startswith('podman') or is_apple(engine) else run_user()
    home = ['-e', 'HOME=/tmp', '-e', 'UV_CACHE_DIR=/tmp/uv-cache'] if user else []
    return [engine, 'run', '--rm', '--platform', selected_platform, *performance_resources(engine), *user,
            '-v', f'{Path(root).resolve()}:/repo:ro', '-v', f'{Path(cache).resolve()}:/provision',
            '-v', f'{Path(shared).resolve()}:/cache', '-e', 'OXPINYIN_CACHE=/cache',
            '-e', 'PYTHONDONTWRITEBYTECODE=1', *home, '-w', '/repo', tag,
            'uv', 'run', '--no-project', '--managed-python', 'python', '-X', 'lazy_imports=normal',
            '-m', 'tools.dev', 'provision', 'oracle', '--cache-dir', '/provision', *argv]


def provision_oracle(argv, *, root=ROOT, env=None):
    """Provision the oracle cells into the host's provisioning cache, inside the performance image."""
    environment = os.environ if env is None else env
    if argv[:1] in (['-h'], ['--help']):
        print(PROVISION_ORACLE_USAGE, end='')
        return 0
    rest = list(argv)
    while rest:
        option = rest.pop(0)
        if option not in ('--cells', '--jobs') or not rest:
            fail(f'unknown option or missing value: {option} (the cache root is OXPINYIN_CACHE)\n'
                 + PROVISION_ORACLE_USAGE, 2)
        rest.pop(0)
    if environment.get('GITHUB_ACTIONS') == 'true':
        fail('provision-oracle is local-only; never run it on GitHub Actions', 2)
    selected = host_platform()
    if selected is None:
        fail('FAIL: provision-oracle supports only native x86_64 and aarch64', 2)
    engine = select_runtime(env=environment)
    tag = performance_tag(root, selected.split('/', 1)[1])
    if quiet([engine, 'image', 'inspect', tag], environment):
        flags = environment.get('OXPINYIN_CAPTURE_BUILD_FLAGS', '').split()
        build = performance_build_command(engine, root, selected, tag)
        if run(runtime_command([*build[:2], *flags, *build[2:]]), cwd=root, env=environment, check=False):
            raise Finished(1)
    cache = provision_cache(environment, root)
    cache.mkdir(parents=True, exist_ok=True)
    print(f'provisioning cache: {cache.resolve()}', file=sys.stderr, flush=True)
    return run(runtime_command(provision_oracle_command(engine, root, cache, tag, selected, argv,
                                                        cachedir.root(environment))),
               cwd=root, env=environment, check=False)


def performance(argv, *, root=ROOT):
    """Build and enter the local performance environment, on the host architecture."""
    parser = argparse.ArgumentParser(prog='just performance')
    parser.add_argument('--engine', choices=('container', 'docker', 'podman', 'wslc'))
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
    tag = performance_tag(root, native)
    resources = performance_resources(engine)
    if not args.skip_build:
        run(runtime_command(performance_build_command(engine, root, selected_platform, tag)))
    if args.build_only:
        return 0
    command = args.command
    if command[:1] == ['--']:
        command = command[1:]
    cache = args.volume or f'oxpinyin-performance-{selected_platform.split("/")[1]}'
    flags = ['-it'] if not command and sys.stdin.isatty() else []
    flags += resources
    common = text(['git', '-C', root, 'rev-parse', '--path-format=absolute', '--git-common-dir'])
    if common.startswith('/'):
        # The worktree's .git names this path; a Windows path (C:/...) is no container path, so
        # git in the container cannot follow it there and the mount is left out.
        flags += ['-v', f'{common}:{common}:ro']
    return run(runtime_command([engine, 'run', '--rm', '--platform', selected_platform, *flags,
                                '-v', f'{Path(root).resolve()}:/repo:ro', '-v', f'{cache}:/performance',
                                '-e', 'CARGO_TARGET_DIR=/performance/target', '-e', 'TMPDIR=/performance/tmp',
                                '-e', 'OXPINYIN_CACHE=/performance/cache',
                                '-e', 'PYTHONDONTWRITEBYTECODE=1', '-w', '/repo', tag, *(command or ['bash'])]),
               check=False)


def check_reproducible(argv, *, root=ROOT):
    """Build twice in distinct paths and compare only those two builds."""
    from tools.dev.measure.recipes import require_performance_environment
    require_performance_environment(os.environ)
    parser = argparse.ArgumentParser(prog='check-reproducible')
    parser.add_argument('--cells', nargs='+', choices=cell.CELLS, default=list(cell.CELLS))
    parser.add_argument('--work-dir', type=Path, required=True)
    parser.add_argument('--jobs', default='2')
    args = parser.parse_args(argv)
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
                run([sys.executable, Path(root) / 'tools/oracle/build.py', '--libs-only', '--enable-libzhuyin',
                     '--dbm', backend, '--work-dir', directory / 'build', '--prefix', prefix,
                     '--jobs', args.jobs], heavy=True)
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
        if args[:1] == ['provision-oracle']:
            return provision_oracle(args[1:])
        if args[:1] == ['image-tag']:
            print(image_tag())
            return 0
        if args[:1] == ['pin-field']:
            return pin_field(args[1:])
        if args[:1] == ['verify-image']:
            return verify_image(args[1:])
        if args[:1] == ['build-image']:
            return build_image(args[1:])
        if args[:1] == ['capture-pin']:
            return capture_pin(args[1:])
        if args[:1] == ['run-in-capture-image'] and len(args) >= 3:
            return run_in_capture_image(args[1], args[2:])
        if args[:1] == ['image-field'] and len(args) == 4:
            print(image_field(*args[1:]))
            return 0
        print('usage: python -m tools.oracle.image '
              'image-tag|pin-field|verify-image|build-image|image-field|capture-pin|run-in-capture-image|provision-oracle|performance ...',
              file=sys.stderr)
        return 2
    return guarded(call)


if __name__ == '__main__':
    sys.exit(main())
