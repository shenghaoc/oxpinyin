"""Content identities for container inputs, built artifacts and exported results.

May run as /input/identity.py before extracting source. Standard library only.
Native builds do not consume these records. Digests detect stale/mixed inputs;
they are not signatures or a sandbox against a malicious source checkout.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import sys
import tarfile


def digest_file(path):
    """Return the hexadecimal SHA-256 of file bytes; read errors propagate."""
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def digest_record(record):
    """Hash a record encoded as sorted, compact JSON with UTF-8."""
    return hashlib.sha256(json.dumps(record, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def read_record(path):
    """Read a JSON object with schema 1, rejecting other shapes or schemas.

    Return the object; file and JSON parsing errors propagate.
    """
    record = json.loads(Path(path).read_text())
    if not isinstance(record, dict) or record.get('schema') != 1:
        raise ValueError(f'missing or unsupported container identity: {path}')
    return record


def write_record(path, record):
    """Overwrite a file with sorted, indented JSON and one trailing newline."""
    Path(path).write_text(json.dumps(record, sort_keys=True, indent=2) + '\n')


# The container volume: the trusted base of every container namespace.
STATE_ROOT = Path('/state')


def unredirected(path, base, *, message=None):
    """Resolve the trusted base, then refuse links in the owned namespace below it."""
    path = Path(path).absolute()
    base = Path(base).absolute()
    relative = path.relative_to(base)
    current = base.resolve()
    for part in relative.parts:
        current = current / part
        if current.is_symlink():
            raise ValueError(message or f'container namespace redirects through a symlink: {path}')
    return current


def result_directory(repo, source, run_id):
    """Return a result path for a SHA-256 source and timestamp/PID run ID.

    Reject invalid identifiers or symlinks below repo; do not create directories.
    """
    if not re.fullmatch(r'[a-f0-9]{64}', source) or not re.fullmatch(r'[0-9]{8}T[0-9]{6}Z-[0-9]+', run_id):
        raise ValueError('invalid container result namespace')
    return unredirected(Path(repo) / '.oxpinyin-container/results' / source / run_id, repo)


def verify_state(image, source, cell, run_id, root=STATE_ROOT):
    """Validate container namespace identifiers and reject redirected state paths.

    Check paths below root before writes; raise ValueError for invalid names
    or symlinks, without creating the paths.
    """
    # The cell and run patterns are compat-inside.sh's; this runs before its first write.
    if (not re.fullmatch(r'[a-f0-9]{16}', image) or not re.fullmatch(r'[a-f0-9]{64}', source)
            or not re.fullmatch(r'(bdb|kc|tkrzw)', cell)
            or not re.fullmatch(r'[0-9]{8}T[0-9]{6}Z-[0-9]+', run_id)):
        raise ValueError('invalid container state namespace')
    root = Path(root)
    for relative in ('work', 'cache', 'model20', f'results/container/{source}/{run_id}',
                     *(f'container/{image}/{name}' for name in
                       ('home', 'cargo', 'uv/cache', 'locks', 'cache', f'targets/{source}/{cell}'))):
        unredirected(root / relative, root)


def state_paths(inputs):
    """The state paths one image and source own; the guest exports and the host checks them."""
    base = '/state/container/' + inputs['image']
    return dict(HOME=base + '/home', CARGO_HOME=base + '/cargo', RUSTUP_HOME='/opt/rustup',
                UV_PYTHON_INSTALL_DIR='/opt/uv/python', UV_CACHE_DIR=base + '/uv/cache',
                UV_INSTALL_DIR='/opt/uv/bin', XDG_CACHE_HOME=base + '/cache',
                OXPINYIN_LOCK_DIR=base + '/locks',
                OXPINYIN_TARGET_ROOT=base + '/targets/' + inputs['source_sha256'])


def inner_state(inputs):
    """Validate image-owned environment paths and interpreter, then return them.

    Raise ValueError if any expected variable differs or the resolved
    interpreter is outside /opt/uv/python.
    """
    expected = state_paths(inputs)
    for name, value in expected.items():
        if os.environ.get(name) != value:
            raise ValueError(f'container state mismatch: {name}')
    executable = Path(sys.executable).resolve()
    if not executable.is_relative_to('/opt/uv/python'):
        raise ValueError('container interpreter is not image-owned')
    return dict(expected, executable=str(executable))


def archive_content(archive):
    """Hash file content/modes and link targets; reject ambiguous extraction."""
    records = {}
    options = dict(fileobj=archive) if hasattr(archive, 'read') else dict(name=archive)
    with tarfile.open(mode='r:', **options) as source:
        for member in source:
            name = PurePosixPath(member.name)
            if name.is_absolute() or '..' in name.parts or str(name) in ('.', ''):
                raise ValueError(f'unsafe source member: {member.name}')
            if str(name) in records or '.git' in name.parts:
                raise ValueError(f'duplicate or Git metadata source member: {member.name}')
            if member.isfile():
                with source.extractfile(member) as stream:
                    value = hashlib.file_digest(stream, 'sha256').hexdigest()
                kind = 'file'
            elif member.issym():
                target = PurePosixPath(member.linkname)
                if target.is_absolute():
                    raise ValueError(f'absolute source symlink: {member.name}')
                depth = len(name.parent.parts)
                for part in target.parts:
                    depth += -1 if part == '..' else 1
                    if depth < 0:
                        raise ValueError(f'escaping source symlink: {member.name}')
                kind, value = 'symlink', member.linkname
            elif member.isdir():
                kind, value = 'directory', ''
            else:
                raise ValueError(f'unsupported source member: {member.name}')
            records[str(name)] = dict(kind=kind, mode=member.mode & 0o777, content=value)
    for name in records:
        for parent in PurePosixPath(name).parents:
            if str(parent) in records and records[str(parent)]['kind'] != 'directory':
                raise ValueError(f'source member beneath non-directory: {name}')
    return records


def seal_inputs(directory, image, reference, head):
    """Validate staged source/helpers and write their container input identity.

    Hash source.tar, model.tar.gz, inside.sh, bash-floor.sh, identity.py, and guest.py;
    a nonempty model must match the archived oracle pin. Write identity.json
    and return its SHA-256. Raise ValueError for mismatches; read/archive
    errors propagate.
    """
    directory = Path(directory)
    files = archive_content(directory / 'source.tar')
    model = directory / 'model.tar.gz'
    if model.stat().st_size:
        with tarfile.open(directory / 'source.tar', 'r:') as archive:
            pin = archive.extractfile('tools/oracle/oracle-pin.txt').read().decode()
        expected = dict(line.split('=', 1) for line in pin.splitlines()
                        if line and not line.startswith('#'))['model_archive_sha256']
        if digest_file(model) != expected:
            raise ValueError('copied model archive checksum mismatch')
    record = dict(schema=1, environment='container', image=image, reference=reference,
                  head=head, source_sha256=digest_record(files), files=files,
                  archive_sha256=digest_file(directory / 'source.tar'),
                  model_sha256=digest_file(model),
                  inside_sha256=digest_file(directory / 'inside.sh'),
                  floor_sha256=digest_file(directory / 'bash-floor.sh'),
                  verifier_sha256=digest_file(directory / 'identity.py'),
                  guest_sha256=digest_file(directory / 'guest.py'))
    for source, field in (('tools/oracle/compat-inside.sh', 'inside_sha256'),
                          ('tools/lib/bash-floor.sh', 'floor_sha256'),
                          ('tools/dev/compat/identity.py', 'verifier_sha256'),
                          ('tools/dev/compat/guest.py', 'guest_sha256')):
        if files.get(source, {}).get('content') != record[field]:
            raise ValueError(f'launch helper differs from source snapshot: {source}')
    write_record(directory / 'identity.json', record)
    return digest_file(directory / 'identity.json')


def verify_inputs(directory, expected, image=None, reference=None):
    """Return the input identity after checking its digest and all staged content.

    Optionally require the image and reference namespaces. Raise ValueError
    for identity/content mismatches; file and archive errors propagate.
    """
    directory = Path(directory)
    if digest_file(directory / 'identity.json') != expected:
        raise ValueError('container input identity mismatch')
    record = read_record(directory / 'identity.json')
    if record.get('environment') != 'container':
        raise ValueError('not a container input identity')
    if image is not None and record.get('image') != image:
        raise ValueError('container image namespace mismatch')
    if reference is not None and record.get('reference') != reference:
        raise ValueError('container reference namespace mismatch')
    for filename, field in (('source.tar', 'archive_sha256'), ('model.tar.gz', 'model_sha256'),
                            ('inside.sh', 'inside_sha256'), ('bash-floor.sh', 'floor_sha256'),
                            ('identity.py', 'verifier_sha256'), ('guest.py', 'guest_sha256')):
        if digest_file(directory / filename) != record.get(field):
            raise ValueError(f'container input content mismatch: {filename}')
    files = archive_content(directory / 'source.tar')
    if files != record.get('files') or digest_record(files) != record.get('source_sha256'):
        raise ValueError('source content identity mismatch')
    return record


def verify_tree(root, files):
    """Check recorded files and symlinks against an extracted source tree.

    Verify file bytes/modes and link targets, rejecting escaping regular
    files with ValueError. Directory records and unlisted entries are not
    checked; read errors propagate.
    """
    root = Path(root).resolve()
    for name, info in files.items():
        path = root / name
        if info['kind'] == 'symlink':
            if not path.is_symlink() or os.readlink(path) != info['content']:
                raise ValueError(f'source symlink mismatch: {name}')
        elif info['kind'] == 'file':
            if path.is_symlink() or not path.resolve().is_relative_to(root):
                raise ValueError(f'source path escaped snapshot: {name}')
            if digest_file(path) != info['content'] or path.stat().st_mode & 0o777 != info['mode']:
                raise ValueError(f'source content mismatch: {name}')


def current_inputs():
    """Verify /input using the digest and namespace IDs from the environment.

    Raise ValueError for a missing digest or failed verification; missing
    namespace variables raise KeyError.
    """
    expected = os.environ.get('OXPINYIN_COMPAT_INPUT_SHA256')
    if not expected:
        raise ValueError('missing container input identity')
    return verify_inputs('/input', expected, os.environ['OXPINYIN_COMPAT_IMAGE_ID'],
                         os.environ['OXPINYIN_COMPAT_REFERENCE_ID'])


def artifact_content(artifacts, target):
    """Return artifact paths and SHA-256 hashes within the container target.

    Reject target namespace symlinks or artifacts resolving outside it with
    ValueError. Read errors propagate.
    """
    target = unredirected(target, STATE_ROOT)
    result = {}
    for name, info in artifacts.items():
        path = Path(info['path'])
        if not path.resolve().is_relative_to(target):
            raise ValueError(f'artifact outside container target: {name}')
        result[name] = dict(path=str(path), sha256=digest_file(path))
    return result


def seal_artifacts(record, target):
    """Return a container provenance record for built artifacts and source.

    Verify current inputs and the recorded worktree before hashing artifacts
    under target. This returns the record without writing it.
    """
    inputs = current_inputs()
    verify_tree(record['worktree'], inputs['files'])
    return dict(schema=1, environment='container', source_sha256=inputs['source_sha256'],
                image=inputs['image'], reference=inputs['reference'], cell=record['cell'],
                profile=record['profile'], toolchain=record['toolchain'],
                artifacts=artifact_content(record['artifacts'], target))


def verify_artifacts(record, root, needed):
    """Return sealed artifact provenance after checking inputs, source, and needed files.

    Require matching cell, profile, and toolchain plus each requested path
    and digest under OXPINYIN_TARGET_ROOT; raise ValueError on mismatch.
    """
    inputs = current_inputs()
    verify_tree(root, inputs['files'])
    sealed = record.get('container')
    if not isinstance(sealed, dict):
        raise ValueError('missing container artifact content identity; rebuild in the container')
    for key in ('source_sha256', 'image', 'reference'):
        if sealed.get(key) != inputs[key]:
            raise ValueError(f'container artifact {key} mismatch')
    for key in ('cell', 'profile', 'toolchain'):
        if sealed.get(key) != record.get(key):
            raise ValueError(f'container artifact {key} mismatch')
    target = unredirected(os.environ['OXPINYIN_TARGET_ROOT'], STATE_ROOT)
    for name, path in needed.items():
        if not Path(path).resolve().is_relative_to(target):
            raise ValueError(f'artifact outside container target: {name}')
        if sealed.get('artifacts', {}).get(name) != dict(path=str(path), sha256=digest_file(path)):
            raise ValueError(f'container artifact content mismatch: {name}')
    return sealed


def seal_results(directory, target, cell):
    """Write provenance.json binding result files to container inputs and state.

    Verify any existing artifact identity and reject PASS rows whose required
    artifacts are absent. Hash result files, excluding provenance.json itself;
    invalid identities raise ValueError and file/catalog errors propagate.
    """
    inputs = current_inputs()
    directory = Path(directory)
    rows = json.loads((directory / 'results.json').read_text())
    # A failed build may have no artifact record; it must never supply a PASS.
    path = Path(target) / cell / 'oxpinyin-cell-identity.json'
    artifacts = None
    if path.is_file():
        record = read_record(path)
        # A failed first build leaves only the unsealed, empty claim: it seals nothing.
        if 'container' in record or record.get('artifacts') != {}:
            artifacts = verify_artifacts(record, record['worktree'],
                                         {name: info['path'] for name, info in record['artifacts'].items()})
    from tools.dev import registry
    catalog = registry.by_name(registry.load())
    for row in rows:
        needed = catalog[row['runner']]['artifacts']
        if row['status'] == 'PASS' and needed and (artifacts is None or
                                                any(name not in artifacts['artifacts'] for name in needed)):
            raise ValueError('passing result lacks container artifact identity')
    files = result_files(directory)
    record = dict(schema=1, environment='container', inputs=inputs,
                  input_sha256=os.environ['OXPINYIN_COMPAT_INPUT_SHA256'],
                  artifacts=artifacts, artifacts_sha256=digest_record(artifacts), files=files,
                  state=inner_state(inputs))
    write_record(directory / 'provenance.json', record)


def verify_results(directory, expected):
    """Validate exported provenance against the expected input digest and result files.

    Check recorded state paths, interpreter namespace, artifact-record digest,
    and every result file hash. Raise ValueError for a mismatch.
    """
    directory = Path(directory)
    record = read_record(directory / 'provenance.json')
    if record.get('environment') != 'container' or record.get('input_sha256') != expected:
        raise ValueError('exported container result input identity mismatch')
    inputs = record.get('inputs')
    encoded = json.dumps(inputs, sort_keys=True, indent=2) + '\n'
    if hashlib.sha256(encoded.encode()).hexdigest() != expected:
        raise ValueError('exported container input content mismatch')
    state = record.get('state')
    executable = state.get('executable') if isinstance(state, dict) else None
    try:
        paths = state_paths(inputs)
    except (KeyError, TypeError):
        paths = None
    interpreter = PurePosixPath(executable) if isinstance(executable, str) else None
    if (paths is None or interpreter is None or not interpreter.is_absolute() or '..' in interpreter.parts
            or not interpreter.is_relative_to('/opt/uv/python')
            or state != dict(paths, executable=executable)):
        raise ValueError('exported container state mismatch')
    if digest_record(record.get('artifacts')) != record.get('artifacts_sha256'):
        raise ValueError('exported container artifact identity mismatch')
    actual = result_files(directory)
    if actual != record.get('files'):
        raise ValueError('exported container result content mismatch')


def result_files(directory):
    """Hash every regular result file except the top-level provenance.json.

    Return relative-path keys and reject any encountered symlink with ValueError.
    """
    files = {}
    for path in Path(directory).rglob('*'):
        if path.is_symlink():
            raise ValueError('container results must not contain symlinks')
        if path.is_file() and path != Path(directory) / 'provenance.json':
            files[str(path.relative_to(directory))] = digest_file(path)
    return files


def main(argv=None):
    """Dispatch container identity actions, printing returned digests or paths.

    Return zero on success or print FAIL and return 2 for handled input,
    filesystem, and archive errors; argument parsing may exit separately.
    """
    parser = argparse.ArgumentParser()
    parser.add_argument('action', choices=('seal-inputs', 'verify-inputs', 'seal-results', 'verify-results',
                                           'result-directory', 'verify-state'))
    parser.add_argument('args', nargs='+')
    args = parser.parse_args(argv)
    try:
        if args.action == 'seal-inputs':
            print(seal_inputs(*args.args))
        elif args.action == 'verify-inputs':
            print(verify_inputs(*args.args)['source_sha256'])
        elif args.action == 'seal-results':
            seal_results(*args.args)
        elif args.action == 'result-directory':
            print(result_directory(*args.args))
        elif args.action == 'verify-state':
            verify_state(*args.args)
        else:
            verify_results(*args.args)
    except (OSError, ValueError, KeyError, TypeError, tarfile.TarError) as exc:
        print(f'FAIL: container identity: {exc}', file=sys.stderr)
        return 2
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
