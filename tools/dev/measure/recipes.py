"""The measurement and profile recipes behind `python -m tools.dev measure` (#1290).

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


# The names `measure` accepts besides its own startup and cold-qualify. A fixed
# tuple keeps every `tools.dev` command from importing the recipe modules;
# recipes.test.py checks it against what the modules contribute.
NAMES = ('callgrind-control', 'callgrind-differential', 'perf-baseline', 'perf-matrix', 'perf-same-data',
         'probe-debuginfo-neutrality', 'probe-egress', 'profile-w8', 'rss-diag',
         'shipped-check', 'snapshot', 'stack-comparison', 'steady-comparison', 'system-import-profile',
         'tree-recipe-control')


def table() -> dict:
    """Every recipe by name; the modules load on first use."""
    from . import campaigns, probes, profile, snapshot, valgrind
    recipes = {}
    for module in (campaigns, probes, profile, snapshot, valgrind):
        recipes.update(module.RECIPES)
    return recipes


PIN_COMPARISONS = frozenset(('perf-baseline', 'perf-matrix', 'perf-same-data',
    'callgrind-differential', 'steady-comparison', 'stack-comparison', 'shipped-check',
    'tree-recipe-control', 'rss-diag', 'system-import-profile'))


def require_performance_environment(env):
    """Only pin-comparison campaigns require the local oracle performance stage."""
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
