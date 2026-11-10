"""Run the nonshipping REPL against a disposable copy of the W3 data."""

if __name__ == '__main__':
    from .entry import require_normal_imports

    require_normal_imports()

import argparse
import json
from pathlib import Path
import shutil
import sys
import tempfile

from . import cell, slot
from .runners import guarded, status

ROOT = Path(__file__).resolve().parents[2]
MODES = ('pinyin', 'phrase', 'chewing', 'zhuyin', 'parser2', 'phrase_table',
         'chewing_table', 'matrix', 'pinyin_lookup', 'phrase_lookup')
BACKENDS = cell.BACKEND_EXTENSIONS


def main(argv):
    """Run with shared cancellation and error handling."""
    return guarded(lambda: run(argv))


def run(argv):
    """Build one selected backend and keep all session writes in a temporary copy."""
    parser = argparse.ArgumentParser(prog='just repl')
    parser.add_argument('mode', choices=MODES)
    parser.add_argument('--backend', choices=BACKENDS, default='tkrzw')
    args, mode_args = parser.parse_known_args(argv)
    if mode_args[:1] == ['--']:
        mode_args = mode_args[1:]
    build = ['cargo', 'build', '--locked', '-p', 'oxpinyin-repl',
             '--no-default-features', '--features', args.backend]
    build.append('--message-format=json-render-diagnostics')
    with tempfile.TemporaryFile(mode='w+', encoding='utf-8') as artifacts:
        result = slot.run_command(build, cwd=ROOT, stdout=artifacts)
        if result:
            return status(result)
        artifacts.seek(0)
        executable = None
        for line in artifacts:
            message = json.loads(line)
            if (message.get('reason') == 'compiler-artifact'
                    and message.get('target', {}).get('name') == 'oxpinyin-repl'
                    and message.get('executable')):
                executable = message['executable']
        if executable is None:
            print('REPL build produced no executable', file=sys.stderr)
            return 2
    with tempfile.TemporaryDirectory(prefix='oxpinyin-repl-') as temporary:
        data = Path(temporary) / 'data'
        shutil.copytree(ROOT / 'fixtures' / 'w3' / BACKENDS[args.backend], data)
        if args.mode in ('phrase_table', 'chewing_table'):
            for table in ('gb_char', 'gbk_char', 'opengram', 'merged'):
                shutil.copy2(ROOT / 'fixtures' / 'datagen-toned' / f'{table}.table', data)
        return status(slot.run_command([str(executable), args.mode, str(data), *mode_args], cwd=temporary))


if __name__ == '__main__':
    raise SystemExit(main(sys.argv[1:]))
