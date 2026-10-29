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
         'probe-debuginfo-neutrality', 'probe-egress', 'processor-trace', 'profile-w8', 'rss-diag',
         'shipped-check', 'snapshot', 'steady-comparison', 'system-import-profile', 'tree-recipe-control')


def table() -> dict:
    """Every recipe by name; the modules load on first use."""
    from . import campaigns, probes, processor_trace, profile, snapshot, valgrind
    recipes = {}
    for module in (campaigns, probes, processor_trace, profile, snapshot, valgrind):
        recipes.update(module.RECIPES)
    return recipes


def main(name: str, argv: Sequence[str], env: Mapping[str, str] | None = None) -> int:
    """Run recipe NAME with ARGV and return its status."""
    recipes = table()
    if name not in recipes:
        runners.emit(f'unknown measure recipe: {name}', error=True)
        return 2
    environment = dict(os.environ if env is None else env)
    status = runners.guarded(lambda: recipes[name](list(argv), environment))
    sys.stdout.flush()
    return status
