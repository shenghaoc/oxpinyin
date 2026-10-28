"""Case registration for the contract differential (contract-diff.py).

Build a case's registration metadata with the shared defaults, check its option
combinations and store it under a unique ID. The registry is a parameter:
contract-diff.py's `case` decorator passes its own CASES when it registers, so a
registry the tests swap in is the one written, and every load of the driver
keeps its own. The option meanings are documented on that decorator.
"""
from contract_selection import normal_tag
from contract_verdict import NO_ABORT


def case_spec(mode='pinyin', control=False, abort=NO_ABORT, stderr=False, crash=None, userdir=False,
              cells=('bdb', 'kc', 'tkrzw'), either=False, host_scanf=(), dir_chunk=None, stderr_varies=False,
              abort_logs=None, fatal=False, both_abort=False, strjoinv_over_read=False, issues=(), *, fn=None,
              policy_record=None, case_id=None, pin_not_run=None):
    """Build the registration metadata with shared defaults, without validating it.

    Store host scanner spellings and normalized issue tags as tuples. A
    crash expectation enables stderr comparison even if stderr is false.
    Registration checks option combinations separately."""
    return dict(fn=fn, mode=mode, control=control, abort=abort, stderr=stderr or crash is not None,
                crash=crash, userdir=userdir, cells=cells, either=either, host_scanf=tuple(host_scanf),
                dir_chunk=dir_chunk, stderr_varies=stderr_varies, abort_logs=abort_logs, fatal=fatal,
                both_abort=both_abort, strjoinv_over_read=strjoinv_over_read,
                issues=tuple(normal_tag(tag) for tag in issues),
                policy_record=policy_record, case_id=case_id, pin_not_run=pin_not_run)


def register(registry, name, fn, *args, **kwargs):
    """Store the case specification in `registry` and return the original function.

    Reject duplicate IDs with ValueError and assert the required option
    combinations before modifying the registry. Policy records cannot be combined
    with the signal, host-dependent or profile-comparison exceptions."""
    if name in registry:
        raise ValueError('duplicate case name: %s' % name)
    spec = case_spec(*args, fn=fn, case_id=name, **kwargs)
    assert not spec['either'] or (spec['abort'] is not NO_ABORT and spec['crash'] is not None), name
    assert not spec['both_abort'] or spec['fatal'], name
    assert spec['dir_chunk'] in (None, 'user', 'system') and (spec['dir_chunk'] is None or spec['abort'] is not NO_ABORT), name
    assert spec['pin_not_run'] is None or not (
        spec['abort'] is not NO_ABORT or spec['crash'] is not None or spec['both_abort'] or spec['either']
        or spec['strjoinv_over_read'] or spec['dir_chunk'] or spec['userdir'] or spec['stderr']
        or spec['policy_record'] is not None), name
    if spec['policy_record'] is not None:
        assert not (spec['abort'] is not NO_ABORT or spec['crash'] is not None
                    or spec['both_abort'] or spec['strjoinv_over_read']
                    or spec['host_scanf'] or spec['dir_chunk'] or spec['userdir']), name
    registry[name] = spec
    return fn
