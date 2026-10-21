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
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def digest_record(record):
    return hashlib.sha256(json.dumps(record, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def read_record(path):
    record = json.loads(Path(path).read_text())
    if not isinstance(record, dict) or record.get('schema') != 1:
        raise ValueError(f'missing or unsupported container identity: {path}')
    return record


def write_record(path, record):
    Path(path).write_text(json.dumps(record, sort_keys=True, indent=2) + '\n')


def unredirected(path):
    path = Path(path).absolute()
    if path.resolve() != path:
        raise ValueError(f'container namespace redirects through a symlink: {path}')
    return path


def result_directory(repo, source, run_id):
    if not re.fullmatch(r'[a-f0-9]{64}', source) or not re.fullmatch(r'[0-9]{8}T[0-9]{6}Z-[0-9]+', run_id):
        raise ValueError('invalid container result namespace')
    return unredirected(Path(repo) / '.oxpinyin-container/results' / source / run_id)


def verify_state(image, source, root=Path('/state')):
    if not re.fullmatch(r'[a-f0-9]{16}', image) or not re.fullmatch(r'[a-f0-9]{64}', source):
        raise ValueError('invalid container state namespace')
    root = Path(root)
    for relative in ('work', 'cache', 'model20', 'results/container/' + source,
                     *(f'container/{image}/{name}' for name in
                       ('home', 'cargo', 'uv/cache', 'locks', 'cache', 'targets/' + source))):
        unredirected(root / relative)


def inner_state(inputs):
    base = '/state/container/' + inputs['image']
    expected = dict(HOME=base + '/home', CARGO_HOME=base + '/cargo', RUSTUP_HOME='/opt/rustup',
                    UV_PYTHON_INSTALL_DIR='/opt/uv/python', UV_CACHE_DIR=base + '/uv/cache',
                    UV_INSTALL_DIR='/opt/uv/bin', XDG_CACHE_HOME=base + '/cache',
                    OXPINYIN_LOCK_DIR=base + '/locks',
                    OXPINYIN_TARGET_ROOT=base + '/targets/' + inputs['source_sha256'])
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
                  verifier_sha256=digest_file(directory / 'identity.py'))
    for source, field in (('tools/oracle/compat-inside.sh', 'inside_sha256'),
                          ('tools/dev/compat/identity.py', 'verifier_sha256')):
        if files.get(source, {}).get('content') != record[field]:
            raise ValueError(f'launch helper differs from source snapshot: {source}')
    write_record(directory / 'identity.json', record)
    return digest_file(directory / 'identity.json')


def verify_inputs(directory, expected, image=None, reference=None):
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
                            ('inside.sh', 'inside_sha256'), ('identity.py', 'verifier_sha256')):
        if digest_file(directory / filename) != record.get(field):
            raise ValueError(f'container input content mismatch: {filename}')
    files = archive_content(directory / 'source.tar')
    if files != record.get('files') or digest_record(files) != record.get('source_sha256'):
        raise ValueError('source content identity mismatch')
    return record


def verify_tree(root, files):
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
    expected = os.environ.get('OXPINYIN_COMPAT_INPUT_SHA256')
    if not expected:
        raise ValueError('missing container input identity')
    return verify_inputs('/input', expected, os.environ['OXPINYIN_COMPAT_IMAGE_ID'],
                         os.environ['OXPINYIN_COMPAT_REFERENCE_ID'])


def artifact_content(artifacts, target):
    target = unredirected(target)
    result = {}
    for name, info in artifacts.items():
        path = Path(info['path'])
        if not path.resolve().is_relative_to(target):
            raise ValueError(f'artifact outside container target: {name}')
        result[name] = dict(path=str(path), sha256=digest_file(path))
    return result


def seal_artifacts(record, target):
    inputs = current_inputs()
    verify_tree(record['worktree'], inputs['files'])
    return dict(schema=1, environment='container', source_sha256=inputs['source_sha256'],
                image=inputs['image'], reference=inputs['reference'], cell=record['cell'],
                profile=record['profile'], toolchain=record['toolchain'],
                artifacts=artifact_content(record['artifacts'], target))


def verify_artifacts(record, root, needed):
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
    target = unredirected(os.environ['OXPINYIN_TARGET_ROOT'])
    for name, path in needed.items():
        if not Path(path).resolve().is_relative_to(target):
            raise ValueError(f'artifact outside container target: {name}')
        if sealed.get('artifacts', {}).get(name) != dict(path=str(path), sha256=digest_file(path)):
            raise ValueError(f'container artifact content mismatch: {name}')
    return sealed


def seal_results(directory, target, cell):
    inputs = current_inputs()
    directory = Path(directory)
    rows = json.loads((directory / 'results.json').read_text())
    # A failed build may have no artifact record; it must never supply a PASS.
    path = Path(target) / cell / 'oxpinyin-cell-identity.json'
    artifacts = None
    if path.is_file():
        record = read_record(path)
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
    directory = Path(directory)
    record = read_record(directory / 'provenance.json')
    if record.get('environment') != 'container' or record.get('input_sha256') != expected:
        raise ValueError('exported container result input identity mismatch')
    inputs = record.get('inputs')
    encoded = json.dumps(inputs, sort_keys=True, indent=2) + '\n'
    if hashlib.sha256(encoded.encode()).hexdigest() != expected:
        raise ValueError('exported container input content mismatch')
    if digest_record(record.get('artifacts')) != record.get('artifacts_sha256'):
        raise ValueError('exported container artifact identity mismatch')
    actual = result_files(directory)
    if actual != record.get('files'):
        raise ValueError('exported container result content mismatch')


def result_files(directory):
    files = {}
    for path in Path(directory).rglob('*'):
        if path.is_symlink():
            raise ValueError('container results must not contain symlinks')
        if path.is_file() and path != Path(directory) / 'provenance.json':
            files[str(path.relative_to(directory))] = digest_file(path)
    return files


def main(argv=None):
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
