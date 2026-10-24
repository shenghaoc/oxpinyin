"""Native nextest and doctest entry points. Standard library only.

The just recipes stay thin. Resolve nextest's concurrency before admission,
then pass its final budget through the same slot implementation as just heavy.
"""
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys
import tomllib

from . import host, slot

ROOT = Path(__file__).resolve().parents[2]


def _strip_separator(args):
    """Copy arguments, removing one leading -- separator if present."""
    args = list(args)
    if args[:1] == ['--']:
        args = args[1:]
    return args


def _nextest_args(args):
    """Translate libtest ignored-test switches before the first -- for nextest."""
    translated = []
    for arg in args:
        if arg == '--':
            translated.extend(args[len(translated):])
            break
        if arg == '--include-ignored':
            translated.append('--run-ignored=all')
        elif arg == '--ignored':
            translated.append('--run-ignored=only')
        else:
            translated.append(arg)
    return translated


def _options(args):
    """Read budget options before the nextest/libtest argument boundary.

    Return the normalized option map and whether a custom config/workspace
    path was supplied. Raise ValueError for duplicate or missing budget
    values and combined short options containing -j or -P.
    """
    names = {'--test-threads': 'threads', '--jobs': 'threads',
             '--profile': 'profile', '--build-jobs': 'build_jobs'}
    found = {}
    custom_config = False
    index = 0
    while index < len(args):
        arg = args[index]
        if arg == '--':
            break
        key, equals, value = arg.partition('=')
        name = names.get(key)
        if arg.startswith(('-j', '-P')) and not arg.startswith('--'):
            name = 'threads' if arg[1] == 'j' else 'profile'
            value = arg[2:].removeprefix('=')
            equals = bool(value)
        elif len(arg) > 2 and arg.startswith(('-v', '-q', '-r', '-h')) and any(c in arg[2:] for c in 'jP'):
            raise ValueError('combined short options containing -j or -P are not supported; separate them')
        if name is not None:
            if name in found:
                raise ValueError(f'{key} was specified more than once')
            if not equals:
                index += 1
                if index == len(args) or args[index] == '--':
                    raise ValueError(f'{key} needs a value')
                value = args[index]
            if not value:
                raise ValueError(f'{key} needs a value')
            found[name] = value
        if key in ('--config-file', '--tool-config-file', '--manifest-path', '--workspace-remap'):
            custom_config = True
        index += 1
    return found, custom_config


def _positive_threads(value):
    """Parse a positive nextest thread count bounded by sys.maxsize.

    Raise ValueError for relative counts, num-cpus, or any unsupported value.
    """
    if not re.fullmatch(r'\+?[0-9]+', value) or not 0 < int(value) <= sys.maxsize:
        raise ValueError('nextest thread overrides require a positive integer for memory admission; '
                         'relative counts and num-cpus are not supported as overrides')
    return int(value)


def _profile_threads(profile):
    """Resolve a profile thread count through .config/nextest.toml inheritance.

    Return None for num-cpus or an inherited default without a count. Raise
    ValueError for unknown/cyclic profiles or unsupported counts; file and
    TOML errors propagate.
    """
    with (ROOT / '.config/nextest.toml').open('rb') as source:
        profiles = tomllib.load(source)['profile']
    visited = set()
    current = profile
    while current is not None:
        if current in visited or current not in profiles:
            raise ValueError(f'unknown or cyclic nextest profile: {profile}')
        visited.add(current)
        config = profiles[current]
        if 'test-threads' in config:
            value = config['test-threads']
            if value == 'num-cpus':
                return None
            if type(value) is not int or not 0 < value <= sys.maxsize:
                raise ValueError(f'unsupported test-threads in profile {current}; use a positive integer or num-cpus')
            return value
        current = config.get('inherits', 'default' if current != 'default' else None)
    return None


def _thread_plan(args, environ, profile):
    """Resolve nextest profile, budget options, thread count, and its source.

    CLI counts override environment counts, then repository profiles apply.
    No-capture forces one thread. Custom config/workspace paths require an
    explicit count; ambiguous or unsupported settings raise ValueError.
    """
    options, custom_config = _options(args)
    if profile is not None and 'profile' in options:
        raise ValueError('--profile was specified more than once')
    selected = options.get('profile', profile if profile is not None else environ.get('NEXTEST_PROFILE', 'local'))
    if 'threads' in options:
        count, source = _positive_threads(options['threads']), 'user-set (nextest CLI)'
    elif 'NEXTEST_TEST_THREADS' in environ:
        count, source = _positive_threads(environ['NEXTEST_TEST_THREADS']), 'user-set (NEXTEST_TEST_THREADS)'
    else:
        if custom_config:
            raise ValueError('custom nextest config/workspace paths require an explicit positive '
                             '--test-threads or NEXTEST_TEST_THREADS value for memory admission')
        count = _profile_threads(selected)
        source = f'profile ({selected})' if count is not None else 'admitted'
    if any(arg in ('--no-capture', '--nocapture') for arg in args):
        count, source = 1, 'user-set (no-capture)'
    return selected, options, count, source


def _prepare_nextest(count, source):
    """Return an admission callback using the resolved nextest process count.

    When count is None, use the admitted Rust test-thread default; otherwise
    include the explicit count in the returned memory estimate.
    """
    def prepare(environ, resources, *, runner=False):
        """Copy the environment and estimate the resolved nextest thread budget.

        Return child settings, their explanations, and the larger of the heavy
        work estimate and nextest thread estimate, in MiB.
        """
        budget = dict(environ)
        if count is not None:
            budget.setdefault('RUST_TEST_THREADS', str(count))
        child, chosen, estimate = host.prepare_heavy_env(budget, resources, runner=runner)
        threads = count if count is not None else int(child['RUST_TEST_THREADS'])
        child['NEXTEST_TEST_THREADS'] = str(threads)
        chosen['NEXTEST_TEST_THREADS'] = (threads, source, count is not None)
        return child, chosen, max(estimate, threads * host.TEST_THREAD_PEAK_MIB)
    return prepare


def _run(command, label):
    """Print the step and command, execute it, and return its subprocess status.

    Report a missing executable as missing just and return 127.
    """
    print(label, file=sys.stderr, flush=True)
    print('command: ' + shlex.join(command), file=sys.stderr, flush=True)
    try:
        return subprocess.run(command, env=os.environ).returncode
    except FileNotFoundError:
        print('missing just; install it and retry (docs/runbooks/setup.md)', file=sys.stderr, flush=True)
        return 127


def run(args=None, profile=None):
    """Run nextest under the shared slot with its final process budget.

    Default to command-line arguments and the local profile unless overridden.
    Return the raw child status, or print FAIL and return 2 for invalid plans
    or OS/value errors during admission and execution.
    """
    args = _strip_separator(sys.argv[1:] if args is None else args)
    try:
        selected, options, count, source = _thread_plan(args, os.environ, profile)
    except (ValueError, OSError, KeyError) as exc:
        print(f'FAIL: {exc}', file=sys.stderr, flush=True)
        return 2
    args = _nextest_args(args)
    if 'profile' not in options:
        args = ['--profile', selected, *args]
    command = ['cargo', 'nextest', 'run', '--no-fail-fast', *args]
    environ = dict(os.environ)
    if 'build_jobs' in options:
        environ['CARGO_BUILD_JOBS'] = options['build_jobs']
    def report(admitted):
        """Print the selected profile, admitted thread count, and nextest command."""
        print(f'nextest profile: {selected}', file=sys.stderr, flush=True)
        print(f'nextest test threads: {admitted["NEXTEST_TEST_THREADS"]} ({source})',
              file=sys.stderr, flush=True)
        print('command: ' + shlex.join(command), file=sys.stderr, flush=True)
    try:
        return slot.run_command(command, env=environ, out=sys.stderr,
                                prepare_heavy_env=_prepare_nextest(count, source),
                                on_admitted=report)
    except (ValueError, OSError) as exc:
        print(f'FAIL: {exc}', file=sys.stderr, flush=True)
        return 2


def run_doc(args=None):
    """Run cargo test --doc through just heavy. Returns the child status."""
    args = _strip_separator(sys.argv[1:] if args is None else args)
    return _run(['just', 'heavy', '--', 'cargo', 'test', '--doc', *args], 'doctest step')
