"""Exclusive Callgrind attribution."""
import re
from pathlib import Path


def callgrind_costs(text, library, *, preserve_objects=False):
    """Read exclusive functions and inclusive call edges from one profile.

    Only exclusive costs sum to the profile totals. Inclusive function costs
    add outgoing edges and can overlap or recurse; they are never a partition.
    """
    events = re.findall(r'^events:\s*(.*?)\s*$', text, re.M)
    totals = re.findall(r'^totals:\s*(.*?)\s*$', text, re.M)
    if len(events) != 1 or len(totals) != 1:
        raise ValueError('expected one Callgrind events/totals pair')
    names = events[0].split()
    total = [int(value) for value in totals[0].split()]
    total += [0] * (len(names) - len(total))
    positions = 1
    ids = {'ob': {}, 'fn': {}}
    obj = function = '???'
    pending = None
    next_callee_object, callee = None, None
    exclusive, edges = {}, {}
    for line in text.splitlines():
        if line.startswith('positions:'):
            positions = len(line.split(':', 1)[1].split())
            continue
        match = re.match(r'^(c?)(ob|fn)=(?:\((\d+)\))?\s*(.*)$', line)
        if match:
            table = ids[match[2]]
            value = match[4]
            if match[3] is not None:
                if value:
                    table[match[3]] = value
                elif match[3] not in table:
                    raise ValueError('unresolved Callgrind name ID')
                value = table[match[3]]
            if match[1]:
                if match[2] == 'ob':
                    next_callee_object = value
                else:
                    callee = (obj if next_callee_object is None else next_callee_object, value)
                    next_callee_object = None
            else:
                if match[2] == 'ob':
                    obj = value
                else:
                    function = value
                    next_callee_object, callee = None, None
            continue
        if line.startswith('calls='):
            if pending is not None or callee is None:
                raise ValueError('invalid Callgrind call association')
            pending = (obj, function, *callee)
            continue
        if not re.match(r'^(?:[+\-]?\d+|\*|0x[0-9a-fA-F]+)(?:\s|$)', line):
            continue
        if pending is not None:
            costs = edges.setdefault(pending, [0] * len(names))
            for index, value in enumerate(line.split()[positions:]):
                costs[index] += int(value)
            pending = None
            continue
        costs = exclusive.setdefault((obj, function), [0] * len(names))
        for index, value in enumerate(line.split()[positions:]):
            costs[index] += int(value)
    sums = [sum(costs[index] for costs in exclusive.values()) for index in range(len(names))]
    if pending is not None or sums != total or 'Ir' not in names or not total[names.index('Ir')]:
        raise ValueError('Callgrind exclusive costs do not reconcile with totals')
    def label(path):
        """Name the subject specially and preserve other paths or reduce them to basenames."""
        return 'SUBJECT' if Path(path) == Path(library) else (path if preserve_objects else Path(path).name)
    objects = {}
    for (path, _), costs in exclusive.items():
        target = objects.setdefault(label(path), [0] * len(names))
        for index, value in enumerate(costs):
            target[index] += value
    ir = names.index('Ir')
    functions = sorted(((label(path), name, costs[ir]) for (path, name), costs in exclusive.items() if costs[ir]),
                       key=lambda item: (-item[2], item[0], item[1]))
    inclusive = {key: list(value) for key, value in exclusive.items()}
    for (caller_object, caller, _, _), value in edges.items():
        target = inclusive.setdefault((caller_object, caller), [0] * len(names))
        for index, count in enumerate(value):
            target[index] += count
    return dict(totals=dict(zip(names, total, strict=True)),
                objects={key: dict(zip(names, value, strict=True)) for key, value in sorted(objects.items())},
                functions=[dict(object=obj, function=name, Ir=value) for obj, name, value in functions],
                inclusive_functions=[dict(object=label(obj), function=fn, Ir=value[ir])
                    for (obj, fn), value in sorted(inclusive.items(), key=lambda item: -item[1][ir]) if value[ir]],
                edges=[dict(caller_object=label(obj), caller=fn, callee_object=label(cob), callee=cfn, Ir=value[ir])
                    for (obj, fn, cob, cfn), value in sorted(edges.items(), key=lambda item: -item[1][ir]) if value[ir]])


def total_profile(raw, executable, pid):
    """Validate Callgrind identity and return the reconciled Ir summary.

    Decode raw as strict UTF-8. Require unique creator, PID, command, and
    Ir-only events records plus equal summary/totals; mismatches raise
    ValueError. Return total, creator, PID, and decoded raw text.
    """
    text = raw.decode('utf-8', errors='strict')
    for field in ('creator', 'pid', 'cmd', 'events'):
        if len(re.findall(r'^' + field + r':.*$', text, re.M)) != 1:
            raise ValueError('Callgrind identity/events unavailable or ambiguous')
    for pattern in (r'^creator:[ \t]+callgrind-[^\s]+[ \t]*$',
                    r'^pid:[ \t]+' + str(pid) + r'[ \t]*$',
                    r'^cmd:[ \t]+' + re.escape(str(executable)) + r'[ \t]+.+$',
                    r'^events:[ \t]+Ir[ \t]*$'):
        if len(re.findall(pattern, text, re.M)) != 1:
            raise ValueError('Callgrind identity/events unavailable or ambiguous')
    summary = re.findall(r'^summary:\s*(\d+)\s*$', text, re.M)
    totals = re.findall(r'^totals:\s*(\d+)\s*$', text, re.M)
    if len(summary) != 1 or len(totals) != 1 or int(summary[0]) != int(totals[0]):
        raise ValueError('Callgrind summary/totals missing or inconsistent')
    return dict(total=int(summary[0]), creator=re.search(r'^creator:[ \t]+([^\s]+)', text, re.M)[1], pid=pid, raw=text)
