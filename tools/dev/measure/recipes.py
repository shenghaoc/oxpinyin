"""Execution checks shared by the Valgrind recipes."""
import os
import sys

from .. import runners


def require_performance_environment(env):
    """Only pin comparisons require the local oracle performance stage."""
    from pathlib import Path
    from ..runners import Finished
    marker = Path('/usr/local/share/oxpinyin/performance-os-release')
    release = Path('/etc/os-release')
    if (env.get('GITHUB_ACTIONS') == 'true' or not marker.is_file() or not release.is_file()
            or marker.read_bytes() != release.read_bytes()):
        runners.emit('pin comparisons require the local oracle performance stage; '
                     'enter it with just performance', error=True)
        raise Finished(2)


def run(operation, argv=None, env=None, *, pin_comparison=False):
    """Run the operation under the shared cancellation and failure handlers."""
    environment = dict(os.environ if env is None else env)

    def execute():
        if pin_comparison:
            require_performance_environment(environment)
        return operation(list(sys.argv[1:] if argv is None else argv), environment)

    status = runners.guarded(execute)
    sys.stdout.flush()
    return status
