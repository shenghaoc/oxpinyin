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


def main(argv=None):
    """Verify inputs, own the volume lock, provision, compare and seal the results."""
    cell, image, run_id, reference, input_digest, *selection = sys.argv[1:] if argv is None else argv
    spec = importlib.util.spec_from_file_location('compat_identity', Path(__file__).with_name('identity.py'))
    identity = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(identity)
    try:
        inputs = identity.verify_inputs('/input', input_digest, image, reference)
        source = inputs['source_sha256']
        identity.verify_state(image, source, cell, run_id)
    except (OSError, ValueError, KeyError) as error:
        print(f'FAIL: container identity: {error}', file=sys.stderr)
        return 2
    result = Path('/state/results/container') / source / run_id
    for directory in (os.environ['HOME'], os.environ['CARGO_HOME'], os.environ['UV_CACHE_DIR'],
                      os.environ['OXPINYIN_LOCK_DIR'], '/state/work', result):
        Path(directory).mkdir(parents=True, exist_ok=True)
    with Path('/state/run.lock').open('w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        print('Execution environment: ', end='', flush=True)
        subprocess.run(['uname', '-sm'], check=True)
        print(f'Reference/image identity: {image}; cell: {cell}', flush=True)
        sys.stdout.buffer.write(Path('/input/source.txt').read_bytes())
        for path in Path('/state/work').iterdir():
            if path.is_dir() and not path.is_symlink():
                shutil.rmtree(path)
            else:
                path.unlink()
        subprocess.run(['tar', '--same-permissions', '-xf', '/input/source.tar', '-C', '/state/work'], check=True)
        os.chdir('/state/work')
        subprocess.run(['git', 'init', '-q'], check=True)
        print('Linux snapshot Git root: ', end='', flush=True)
        subprocess.run(['git', 'rev-parse', '--show-toplevel'], check=True)
        sys.path.insert(0, '/state/work')
        from tools.dev.compat.process import capture, run, tee
        from tools.dev.runners import guarded
        from tools.dev.compat.identity import seal_results
        from tools.dev import model
        environment = dict(os.environ, OXPINYIN_ORACLE_ENV=f'/state/cache/{reference}/oracle.env',
                           PINYIN_MODEL_CACHE='/state/model20', PINYIN_ORACLE_DBM=cell,
                           OXPINYIN_TARGET_ROOT=f'/state/container/{image}/targets/{source}')
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
            status = tee([sys.executable, '-X', 'lazy_imports=normal', '-m', 'tools.dev', 'runner', 'run-all',
                          '--cells', cell, '--oracle', cell + '=' + environment['PINYIN_ORACLE_PREFIX'],
                          '--target-root', environment['OXPINYIN_TARGET_ROOT'], '--json', str(result / 'results.json'),
                          '--log-dir', str(result / 'logs'), *selection], result / 'comparison.log', env=environment)
            os.environ.update(environment)
            seal_results(result, environment['OXPINYIN_TARGET_ROOT'], cell)
            print(f'Compatibility command exit: {status} (missing/skipped inputs are not passes)')
            return status
        return guarded(execute)


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except subprocess.CalledProcessError as error:
        raise SystemExit(128 - error.returncode if error.returncode < 0 else error.returncode)
