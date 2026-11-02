"""Staged guest handoff: standard library only until the verified tree is extracted.

The Bash bootstrap establishes the image-owned interpreter environment and checks
its staged floor. This helper is bound to the same source identity as inside.sh.
"""
import fcntl
import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def reconcile(archive, work, files, identity):
    """Keep byte-identical source files and replace everything else under the lock."""
    work = Path(work)
    expected = set(files)
    for name in files:
        expected.update(str(parent) for parent in Path(name).parents if str(parent) != '.')
    changed = False
    with tempfile.TemporaryDirectory(prefix='oxpinyin-snapshot-', dir=work.parent) as temporary:
        staged = Path(temporary)
        subprocess.run(['tar', '--same-permissions', '-xf', str(archive), '-C', str(staged)], check=True)
        identity.verify_tree(staged, files)
        for root, dirs, names in os.walk(work, topdown=False, followlinks=False):
            relative_root = Path(root).relative_to(work)
            for name in names + dirs:
                relative = relative_root / name
                if relative.parts[0] == '.git':
                    continue
                path = work / relative
                incoming = staged / relative
                if str(relative) not in expected or not incoming.exists() and not incoming.is_symlink():
                    if path.is_dir() and not path.is_symlink():
                        shutil.rmtree(path)
                    else:
                        path.unlink()
                    changed = True
        for relative in sorted(expected, key=lambda x: (len(Path(x).parts), x)):
            src, dst = staged / relative, work / relative
            if src.is_dir() and not src.is_symlink():
                if dst.is_symlink() or (dst.exists() and not dst.is_dir()):
                    dst.unlink()
                    changed = True
                if not dst.exists():
                    dst.mkdir()
                    changed = True
                mode = src.stat().st_mode & 0o777
                if dst.stat().st_mode & 0o777 != mode:
                    dst.chmod(mode)
                    changed = True
                continue
            same = False
            if src.is_symlink():
                same = dst.is_symlink() and os.readlink(src) == os.readlink(dst)
            elif dst.is_file() and not dst.is_symlink():
                same = (src.stat().st_mode & 0o777 == dst.stat().st_mode & 0o777
                        and identity.digest_file(src) == identity.digest_file(dst))
            if not same:
                if dst.is_dir() and not dst.is_symlink():
                    shutil.rmtree(dst)
                elif dst.exists() or dst.is_symlink():
                    dst.unlink()
                if src.is_symlink():
                    dst.symlink_to(os.readlink(src))
                else:
                    shutil.copyfile(src, dst)
                    dst.chmod(src.stat().st_mode & 0o777)
                changed = True
        identity.verify_tree(work, files)
    return changed


def remove_empty_runner_dirs(work, files):
    """Remove the runner's empty lock directories without treating them as source."""
    if any(name == 'target' or name.startswith('target/') for name in files):
        return
    target = Path(work) / 'target'
    locks = target / 'runner-locks'
    if target.is_symlink() or locks.is_symlink() or not locks.is_dir():
        return
    try:
        locks.rmdir()
        target.rmdir()
    except OSError:
        # Anything else is handled by reconciliation.
        pass


def prepare_source_target(target):
    """Discard targets left by a previous interrupted run of this source."""
    target = Path(target)
    marker = target / '.compat-incomplete'
    if marker.is_symlink():
        raise ValueError('target interruption marker is a symlink')
    if marker.exists():
        print('Compatibility target invalidated: interrupted previous run', flush=True)
        shutil.rmtree(target)
    target.mkdir(parents=True, exist_ok=True)
    marker.touch()
    return marker


def finish_source_target(marker, status):
    """A completed and sealed run retains its target even if tests failed."""
    Path(marker).unlink()


def main(argv=None):
    """Verify inputs, own the volume lock, provision, compare and seal the results."""
    # Python imports must not dirty the reconciled source snapshot between runs.
    sys.dont_write_bytecode = True
    os.environ['PYTHONDONTWRITEBYTECODE'] = '1'
    cell, image, run_id, reference, input_digest, *selection = sys.argv[1:] if argv is None else argv
    spec = importlib.util.spec_from_file_location('compat_identity', Path(__file__).with_name('identity.py'))
    identity = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(identity)
    try:
        inputs = identity.verify_inputs('/input', input_digest, image, reference)
        source = inputs['source_sha256']
        if not inputs.get('image_content') or inputs.get('backend') != cell:
            raise ValueError('missing or mismatched build environment identity')
        identity.verify_state(image, source, cell, run_id, inputs=inputs)
    except (OSError, ValueError, KeyError) as error:
        print(f'FAIL: container identity: {error}', file=sys.stderr)
        return 2
    result = Path('/state/results/container') / source / run_id
    for directory in (os.environ['HOME'], os.environ['CARGO_HOME'], os.environ['UV_CACHE_DIR'],
                      os.environ['OXPINYIN_LOCK_DIR'], '/state/work', result):
        Path(directory).mkdir(parents=True, exist_ok=True)
    with Path('/state/run.lock').open('w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        target_root = identity.state_paths(inputs)['OXPINYIN_TARGET_ROOT']
        marker = prepare_source_target(target_root)
        print('Execution environment: ', end='', flush=True)
        subprocess.run(['uname', '-sm'], check=True)
        print(f'Reference/image identity: {image}; cell: {cell}', flush=True)
        sys.stdout.buffer.write(Path('/input/source.txt').read_bytes())
        remove_empty_runner_dirs('/state/work', inputs['files'])
        reconcile('/input/source.tar', '/state/work', inputs['files'], identity)
        git_dir = Path('/state/work/.git')
        if git_dir.is_dir() and not git_dir.is_symlink():
            shutil.rmtree(git_dir)
        elif git_dir.exists() or git_dir.is_symlink():
            git_dir.unlink()
        os.chdir('/state/work')
        subprocess.run(['git', 'init', '-q'], check=True)
        print('Linux snapshot Git root: ', end='', flush=True)
        subprocess.run(['git', 'rev-parse', '--show-toplevel'], check=True)
        sys.path.insert(0, '/state/work')
        from tools.dev.compat.process import capture, run, tee
        from tools.dev.runners import guarded
        from tools.dev.compat.identity import seal_results
        from tools.dev import model
        from tools.dev.compat import boundary, prepared
        environment = dict(os.environ, OXPINYIN_ORACLE_ENV=f'/state/cache/{reference}/oracle.env',
                           PINYIN_MODEL_CACHE='/state/model20', PINYIN_ORACLE_DBM=cell,
                           OXPINYIN_TARGET_ROOT=target_root)
        Path(environment['OXPINYIN_ORACLE_ENV']).parent.mkdir(parents=True, exist_ok=True)

        def execute():
            if Path('/input/model.tar.gz').stat().st_size:
                print(model.fetch(environment['PINYIN_MODEL_CACHE'], archive='/input/model.tar.gz', env=environment))
            run(['bash', 'tools/cloud/setup.sh', '--in-container', '--cells', cell,
                 '--cache-dir', f'/state/cache/{reference}', '--jobs', environment.get('CARGO_BUILD_JOBS', '2')], env=environment)
            done = capture(['bash', '-e', '-u', '-c', 'source "$1"; env -0', '--', environment['OXPINYIN_ORACLE_ENV']], env=environment)
            environment.update(os.fsdecode(entry).split('=', 1) for entry in done.stdout.split(b'\0') if entry)
            print('Subject Rust toolchain: ', end='', flush=True)
            run(['rustup', 'show', 'active-toolchain'], env=environment)
            rows = boundary.validate(['--cell', cell, *selection], environ={}).rows
            prepared_options = prepared.prepare(
                cell, rows, Path(environment['PINYIN_ORACLE_PREFIX']),
                Path(f'/state/container/{image}/prepared'), environment)
            status = tee([sys.executable, '-X', 'lazy_imports=normal', '-m', 'tools.dev', 'runner', 'run-all',
                          '--cells', cell, '--oracle', cell + '=' + environment['PINYIN_ORACLE_PREFIX'],
                          '--target-root', environment['OXPINYIN_TARGET_ROOT'], '--json', str(result / 'results.json'),
                          '--log-dir', str(result / 'logs'), *prepared_options, *selection], result / 'comparison.log', env=environment)
            os.environ.update(environment)
            seal_results(result, environment['OXPINYIN_TARGET_ROOT'], cell)
            finish_source_target(marker, status)
            print(f'Compatibility command exit: {status} (missing/skipped inputs are not passes)')
            return status
        return guarded(execute)


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except subprocess.CalledProcessError as error:
        raise SystemExit(128 - error.returncode if error.returncode < 0 else error.returncode)
