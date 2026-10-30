#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Select the verification cells for a diff: docs/runbooks/verification.md, rule 2.

Runs rule 2's three checks on BASE...HEAD and prints the cells to run, which check
fired and the matching paths and lines, as plain text for a PR body. A check that
fires selects all three cells; no check selects the reference cell alone.

  1. The diff lists a file under crates/oxpinyin-store/.
  2. A changed non-Markdown file contains feature = "<backend>", for bdb, tkrzw or
     kyotocabinet, with optional spaces around the =.
  3. The diff adds or removes a line that names a backend in any Cargo.toml.

Check 3 reads the added and removed lines of a -U0 diff. Once a hunk has started
every +/- line is content, so a name at the start of a line counts, and the ---/+++
file headers, which carry paths and not lines, never do. Check 2 reads each changed
file as committed at HEAD. Diffs use --no-renames: both paths of a move count, and
the host's diff.renames setting cannot change the answer.

Git reads a pathspec relative to the directory it runs in, so every git command runs
from the top of the work tree (git rev-parse --show-toplevel). The answer does not
depend on the directory the script is run from, and the script reads no file itself.

--run-flag prints one line instead, run=true when a check fired or the diff touches a
--also pathspec and run=false otherwise, in the key=value form a GitHub Actions step
appends to $GITHUB_OUTPUT. --also names the paths a caller adds to the rule, such as a
workflow's own infrastructure; it needs --run-flag.

Exit status: 0 when the checks ran, whichever cells they selected; 1 when git failed,
BASE is not a commit here, or the directory is not in a git work tree; 2 for a usage
error. Standard library only.

Usage:
    cells-for-diff.py                  # origin/main...HEAD
    cells-for-diff.py BASE             # BASE...HEAD
    cells-for-diff.py --limit 0 BASE   # list every match, not the first 10 per check
    cells-for-diff.py --run-flag --also tools/bdb BASE   # run=true or run=false
"""
import argparse
import os
import re
import subprocess
import sys

# The backends as the checks name them, and the cells a differential runs.
BACKENDS = ('bdb', 'tkrzw', 'kyotocabinet')
CELLS = ('bdb', 'kc', 'tkrzw')
REFERENCE_CELL = 'tkrzw'
STORE_DIR = 'crates/oxpinyin-store/'
# Rule 2's grep -E pattern as written; git grep -E reads the same syntax.
FEATURE_GATE = 'feature *= *"(' + '|'.join(BACKENDS) + ')"'
BACKEND_NAME = re.compile('|'.join(BACKENDS))
# Changed files handed to one git grep, so a large diff stays under the argument limit.
GREP_BATCH = 200


def run_git(*args, repo=None):
    """Run git in repo (default: the current directory) and return the finished process."""
    return subprocess.run(('git', *args), cwd=repo, capture_output=True, check=False,
                          env={**os.environ, 'LC_ALL': 'C'})


def git(*args, repo=None, ok=(0,)):
    """Run git in repo and return its stdout; a status outside ok is an error."""
    result = run_git(*args, repo=repo)
    if result.returncode not in ok:
        detail = result.stderr.decode('utf-8', 'replace').strip()
        raise ValueError(f'git {args[0]} failed ({result.returncode}): {detail}')
    return result.stdout


def work_tree_root(directory=None):
    """The top of the git work tree that directory (default: the current one) is in."""
    result = run_git('rev-parse', '--show-toplevel', repo=directory)
    if result.returncode:
        detail = result.stderr.decode('utf-8', 'replace').strip()
        raise ValueError(f'not inside a git work tree ({detail})')
    return os.fsdecode(result.stdout.rstrip(b'\n'))


def resolve(rev, repo=None):
    """The full hash of the commit rev names."""
    found = b''
    if not rev.startswith('-'):
        found = git('rev-parse', '--verify', '--quiet', f'{rev}^{{commit}}', repo=repo, ok=(0, 1))
    if not found:
        raise ValueError(f'{rev!r} is not a commit here; fetch it or name another base')
    return found.decode().strip()


def changed_paths(base, head, *options, pathspec=(), repo=None):
    """The paths the diff from base to head touches."""
    out = git('diff', '--name-only', '-z', '--no-renames', *options, base, head, '--', *pathspec,
              repo=repo)
    return [os.fsdecode(path) for path in out.split(b'\0') if path]


def literal(path):
    """A pathspec that matches path itself, whatever characters it holds."""
    return f':(literal){path}'


def store_paths(base, head, repo):
    """Check 1: the changed paths under the store crate."""
    return changed_paths(base, head, pathspec=(STORE_DIR,), repo=repo)


def feature_gates(base, head, repo):
    """Check 2: the lines of the changed non-Markdown files, as at head, that gate on a backend.

    A file the diff deletes is not at head and matches nothing.
    """
    paths = changed_paths(base, head, pathspec=(':!*.md',), repo=repo)
    found = []
    for start in range(0, len(paths), GREP_BATCH):
        batch = [literal(path) for path in paths[start:start + GREP_BATCH]]
        out = git('grep', '--no-color', '--no-column', '-z', '-n', '-I', '-E', '-e', FEATURE_GATE,
                  head, '--', *batch, repo=repo, ok=(0, 1))
        for record in out.split(b'\n'):
            if record:
                where, number, text = record.split(b'\0', 2)
                path = os.fsdecode(where).partition(':')[2]
                found.append(f"{path}:{number.decode()}: {text.decode('utf-8', 'replace').strip()}")
    return found


def backend_lines(base, head, repo):
    """Check 3: the added and removed lines of the changed Cargo.toml files that name a backend.

    The paths come from a --no-renames list, so a moved file is a deleted path and an added one.
    """
    found = []
    for path in changed_paths(base, head, pathspec=('*Cargo.toml',), repo=repo):
        patch = git('diff', '-U0', '--no-color', '--no-ext-diff', '--no-textconv', base, head, '--',
                    literal(path), repo=repo)
        in_hunk = False
        for line in patch.decode('utf-8', 'replace').split('\n'):
            if line.startswith('@@'):
                in_hunk = True
            elif in_hunk and line[:1] in ('+', '-') and BACKEND_NAME.search(line[1:]):
                found.append(f'{path}: {line.rstrip()}')
    return found


CHECKS = (
    (1, f'a changed path under {STORE_DIR}', store_paths),
    (2, 'a feature = "<backend>" line in a changed non-Markdown file', feature_gates),
    (3, 'a changed Cargo.toml line that names a backend', backend_lines),
)


def run_checks(base, repo=None):
    """Apply the three checks to base...HEAD: (merge base, head, [(number, title, matches)]).

    repo is any directory in the work tree (default: the current one). Every git command
    runs from the top of that work tree, whatever directory repo is.
    """
    root = work_tree_root(repo)
    head = resolve('HEAD', root)
    found = git('merge-base', resolve(base, root), head, repo=root, ok=(0, 1))
    if not found:
        raise ValueError(f'{base!r} and HEAD have no common ancestor')
    merge_base = found.decode().strip()
    return merge_base, head, [(number, title, check(merge_base, head, root))
                              for number, title, check in CHECKS]


def printable(text):
    """Text safe for any stdout encoding: undecodable bytes of a path print as escapes."""
    return text.encode('utf-8', 'surrogateescape').decode('utf-8', 'backslashreplace')


def report(base, merge_base, head, results, limit):
    """The text for a PR body; limit is the most matches shown per check, 0 for all."""
    fired = [str(number) for number, _, matches in results if matches]
    lines = [f'Diff: {base}...HEAD (merge base {merge_base[:8]}, head {head[:8]}).']
    if fired:
        noun = 'check' if len(fired) == 1 else 'checks'
        lines.append(f"Cells: {','.join(CELLS)} ({noun} {', '.join(fired)} fired).")
    else:
        lines.append(f'Cells: {REFERENCE_CELL} (the reference cell; no check fired).')
    lines.append('')
    for number, title, matches in results:
        lines.append(f"Check {number}, {title}: {'fired' if matches else 'not fired'}")
        shown = matches[:limit] if limit else matches
        lines.extend('  ' + printable(match) for match in shown)
        if len(shown) < len(matches):
            lines.append(f'  ... and {len(matches) - len(shown)} more (--limit 0 lists all)')
    return '\n'.join(lines) + '\n'


def run_flag(base, also=(), repo=None):
    """run=true when a check fires for base...HEAD or the diff touches an also pathspec."""
    merge_base, head, results = run_checks(base, repo)
    selected = any(matches for _, _, matches in results)
    if not selected and also:
        selected = bool(changed_paths(merge_base, head, pathspec=also, repo=work_tree_root(repo)))
    return f"run={'true' if selected else 'false'}\n"


def non_negative(value):
    """Convert a CLI integer, rejecting negative values with ArgumentTypeError."""
    number = int(value)
    if number < 0:
        raise argparse.ArgumentTypeError('must not be negative')
    return number


def main(argv=None):
    """Print verification cells selected by checks on the requested Git diff; return zero.

    Git and filesystem errors propagate to the CLI error handler.
    """
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('base', nargs='?', default='origin/main',
                        help='base ref of the diff BASE...HEAD (default: origin/main)')
    parser.add_argument('--limit', type=non_negative, default=10, metavar='N',
                        help='matches shown per check; 0 shows all (default: 10)')
    parser.add_argument('--run-flag', action='store_true',
                        help='print run=true or run=false instead of the report')
    parser.add_argument('--also', action='append', default=[], metavar='PATHSPEC',
                        help='a changed path matching PATHSPEC also sets run=true; needs --run-flag')
    args = parser.parse_args(argv)
    if args.also and not args.run_flag:
        parser.error('--also needs --run-flag')
    if args.run_flag:
        sys.stdout.write(run_flag(args.base, args.also))
        return 0
    merge_base, head, results = run_checks(args.base)
    sys.stdout.write(report(args.base, merge_base, head, results, args.limit))
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, ValueError) as exc:
        print(f'FAIL: {exc}', file=sys.stderr)
        sys.exit(1)
