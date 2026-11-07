"""The Valgrind recipes behind `python -m tools.dev measure` (#1290).

Each module below contributes a RECIPES mapping of recipe name to
`function(argv, env) -> status`. A recipe composes the services in bench.py,
raises `Finished(status)` to stop, and keeps the legacy runner's name in its
diagnostics. This module only selects and runs one, under the cancellation
handlers and failure ladder every runner shares.
"""
import os
import sys
from collections.abc import Mapping, Sequence

from .. import runners


# The names `measure` accepts. A fixed tuple keeps every `tools.dev` command
# from importing the recipe module; recipes.test.py checks it against what the
# module contributes.
NAMES = ('callgrind-control', 'callgrind-differential', 'system-import-profile')


def table() -> dict:
    """Every recipe by name; the module loads on first use."""
    from . import valgrind
    return dict(valgrind.RECIPES)


PIN_COMPARISONS = frozenset(('callgrind-differential', 'system-import-profile'))


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


def main(name: str, argv: Sequence[str], env: Mapping[str, str] | None = None) -> int:
    """Run recipe NAME with ARGV and return its status."""
    recipes = table()
    if name not in recipes:
        runners.emit(f'unknown measure recipe: {name}', error=True)
        return 2
    environment = dict(os.environ if env is None else env)
    def execute():
        if name in PIN_COMPARISONS:
            require_performance_environment(environment)
        return recipes[name](list(argv), environment)
    status = runners.guarded(execute)
    sys.stdout.flush()
    return status
