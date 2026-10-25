"""Shared harness test assertions that never print a call's arguments.

A failing Mock.assert_not_called or assert_called_once_with prints every
recorded call, and assertIn or assertEqual on a mapping prints the mapping.
Launcher, subprocess and HeavyAdmission mocks receive the environment, so one
such failure printed API and GitHub tokens (#1262). These helpers report call
counts and the argument positions or variable names that differ, never values.
Standard library only.
"""


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
