"""Temporary native coverage state; Cargo owns argument and report selection.

Run tests and produce their report in one invocation. No profiles survive
for a later `report` call. The existing slot core owns admission and cleanup
of child processes, including when this runs inside `just heavy`.
"""
import os
import sys
import tempfile

from . import slot


def run(args):
    """Run cargo llvm-cov under admission with temporary build and target paths.

    Forward args unchanged and remove the temporary directory on exit. Return
    the child shell status, 130 on cancellation, or 1 for reported OS/value
    errors; print the temporary target path and failures to stderr.
    """
    try:
        with tempfile.TemporaryDirectory(prefix='oxpinyin-coverage-') as scratch:
            target = os.path.join(scratch, 'target')
            env = dict(os.environ, CARGO_LLVM_COV_TARGET_DIR=target,
                       CARGO_LLVM_COV_BUILD_DIR=target)
            print(f'CARGO_LLVM_COV_TARGET_DIR: {target}', file=sys.stderr, flush=True)
            rc = slot.run_command(['cargo', 'llvm-cov', *args], env=env)
        return 128 - rc if rc < 0 else rc
    except (OSError, ValueError) as exc:
        diagnostic = ' '.join(str(exc).splitlines())
        print(f'coverage: {diagnostic}', file=sys.stderr)
        return 130 if isinstance(exc, slot.Cancelled) else 1
