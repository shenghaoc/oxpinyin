"""Shared harness test helpers and assertions that never print a call's arguments.

A failing Mock.assert_not_called or assert_called_once_with prints every
recorded call, and assertIn or assertEqual on a mapping prints the mapping.
Launcher, subprocess and HeavyAdmission mocks receive the environment, so one
such failure printed API and GitHub tokens (#1262). These helpers report call
counts and the argument positions or variable names that differ, never values.
Standard library only.
"""
import contextlib
import sys
import types
from unittest import mock


@contextlib.contextmanager
def windows_launch(status=0, write=None):
    """Drive slot.run_command's Windows branch on any host, starting nothing.

    Patch sys.platform to win32, admission to a stand-in that holds no lock,
    and the Job Object, ntdll and Popen to recorders. WRITE, if given, fills a
    child's standard output when it is redirected to a file. Yield the Popen
    mock, the jobs created (each with the process handles it was given), and
    the ntdll stand-in.
    """
    from tools.dev import slot

    jobs = []

    class Job:
        def __init__(self):
            self.assigned = []
            jobs.append(self)

        def assign(self, handle):
            self.assigned.append(handle)

        def terminate(self, code=1):
            pass

        def __enter__(self):
            return self

        def __exit__(self, *exc):
            return False

    class Admission:
        def __init__(self, command, env, *args, **options):
            self.env = dict(env)
            # The report's figures, and no descriptor to pass or hand off: the Windows branch needs neither.
            self.slot = types.SimpleNamespace(slot=0)
            self.slot_wait = self.memory_wait = 0.0

        def __enter__(self):
            return self

        def __exit__(self, *exc):
            return False

    def popen(argv, **options):
        if write is not None and hasattr(options.get('stdout'), 'write'):
            write(options['stdout'])
        process = mock.MagicMock(args=argv, _handle=4242)
        process.wait.return_value = status
        return process

    ntdll = mock.Mock()
    ntdll.NtResumeProcess.return_value = 0
    with mock.patch.object(sys, 'platform', 'win32'), \
            mock.patch.object(slot, 'HeavyAdmission', Admission), \
            mock.patch.object(slot, 'WindowsJob', Job), \
            mock.patch('ctypes.WinDLL', return_value=ntdll, create=True), \
            mock.patch.object(slot.subprocess, 'Popen', side_effect=popen) as launch:
        yield launch, jobs, ntdll


def assert_windows_launches(test, launch, jobs, ntdll, cwd=None):
    """Fail unless every launch took run_command's Job Object path without pass_fds."""
    from tools.dev import slot

    test.assertEqual(len(jobs), launch.call_count, 'one Job Object per launch')
    test.assertEqual(ntdll.NtResumeProcess.call_count, launch.call_count, 'each child resumed once')
    for index, call in enumerate(launch.call_args_list):
        test.assertNotIn('pass_fds', call.kwargs, f'launch {index} passed descriptors')
        test.assertNotIn('start_new_session', call.kwargs, f'launch {index} used the POSIX path')
        test.assertEqual(call.kwargs.get('creationflags'), slot.CREATE_SUSPENDED,
                         f'launch {index} not CREATE_SUSPENDED')
        test.assertEqual(call.kwargs.get('cwd'), cwd, f'launch {index} working directory')
        test.assertEqual(jobs[index].assigned, [4242], f'launch {index} not assigned to its job')


def assert_not_called(test, mocked):
    """Fail if mocked was called, reporting only its call count."""
    test.assertEqual(mocked.call_count, 0, f'{mocked!r} was called')


def assert_calls(test, mocked, calls):
    """Fail unless mocked's calls equal calls, a list of mock.call, in order.

    Report the call count, or the positions and keywords at which a call
    differs, never an argument value.
    """
    test.assertEqual(mocked.call_count, len(calls), f'{mocked!r} call count')
    for index, (actual, expected) in enumerate(zip(mocked.call_args_list, calls)):
        test.assertEqual(_differences(actual, expected), [], f'{mocked!r} call {index} differs')


def assert_same_environment(test, actual, expected):
    """Fail unless two environment mappings are equal, naming only the variables that differ."""
    names = sorted(name for name in actual.keys() | expected.keys()
                   if name not in actual or name not in expected or actual[name] != expected[name])
    test.assertEqual(names, [], 'environment variables that differ')


def _differences(actual, expected):
    """List the argument count, positions and keywords at which two calls differ."""
    labels = [] if len(actual.args) == len(expected.args) else ['argument count']
    labels += [f'argument {index}' for index, (left, right) in enumerate(zip(actual.args, expected.args))
               if left != right]
    labels += [f'keyword {name}' for name in sorted(actual.kwargs.keys() | expected.kwargs.keys())
               if name not in actual.kwargs or name not in expected.kwargs
               or actual.kwargs[name] != expected.kwargs[name]]
    return labels


def collect_declarations(*registrars, registry=None, **injected):
    """Collect contract (name, args, metadata, function) declarations for harness assertions.

    Keep registration order and pass injected fixture helpers to each registrar.
    When a family decorates earlier cases during registration, publish each
    declaration into its registry immediately. Return the original function
    from the decorator, just as the driver does.
    """
    cases = []

    def case(name, *args, **metadata):
        def collect(fn):
            cases.append((name, args, metadata, fn))
            if registry is not None:
                registry[name] = dict(metadata, fn=fn)
            return fn
        return collect

    for register in registrars:
        register(case, **injected)
    return cases
