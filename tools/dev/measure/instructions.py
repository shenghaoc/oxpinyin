"""Exclusive Callgrind attribution and the 2026-10-24 RandomState rule."""
import re
from pathlib import Path
import statistics


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
    return dict(totals=dict(zip(names, total)),
                objects={key: dict(zip(names, value)) for key, value in sorted(objects.items())},
                functions=[dict(object=obj, function=name, Ir=value) for obj, name, value in functions],
                inclusive_functions=[dict(object=label(obj), function=fn, Ir=value[ir])
                    for (obj, fn), value in sorted(inclusive.items(), key=lambda item: -item[1][ir]) if value[ir]],
                edges=[dict(caller_object=label(obj), caller=fn, callee_object=label(cob), callee=cfn, Ir=value[ir])
                    for (obj, fn, cob, cfn), value in sorted(edges.items(), key=lambda item: -item[1][ir]) if value[ir]])


RANDOMSTATE_ALLOWLIST = frozenset((
    'dedup_ranked_by_text', 'order_window_tail', '__memcmp_avx2_movbe',
))


def allowed(function):
    """Recognize an exact RandomState allowlisted name after Rust-path normalization.

    Remove a trailing Rust hash and the engine session prefix; clone suffixes
    and arbitrary substring matches remain rejected.
    """
    # Accept complete demangled Rust paths, never a substring or a clone suffix.
    name = re.sub(r'::h[0-9a-f]{16}$', '', function)
    if name.startswith('oxpinyin_engine::session::'):
        name = name.removeprefix('oxpinyin_engine::session::')
    return name in RANDOMSTATE_ALLOWLIST


def resolve_unmapped(text, documents, pid, executable):
    """Split unknown exclusive instruction sites using this process's loader PT_LOADs.

    --dump-instr=yes --compress-pos=no supplies uncompressed object-relative instruction positions.
    A missing/transient/JIT mapping cannot be inferred and therefore refuses.
    """
    objects = [d for d in documents if d.get('startup') == 'loaded_object']
    positions = []
    ids = {'ob': {}, 'fn': {}}
    obj = fn = '???'
    skip = False
    output = []
    for line in text.splitlines():
        if line.startswith('positions:'):
            positions = line.split(':', 1)[1].split()
        match = re.match(r'^(c?)(ob|fn)=(?:\((\d+)\))?\s*(.*)$', line)
        if match:
            prefix, kind, index, value = match.groups()
            if index is not None:
                if value:
                    ids[kind][index] = value
                elif index not in ids[kind]:
                    raise ValueError('unresolved Callgrind name ID')
                value = ids[kind][index]
            if not prefix:
                if kind == 'ob':
                    obj = value
                else:
                    fn = value
        elif line.startswith('calls='):
            skip = True
        elif re.match(r'^(?:[+\-]?\d+|\*|0x[0-9a-fA-F]+)(?:\s|$)', line):
            if skip:
                skip = False
            elif '?' in obj or '?' in fn or re.fullmatch(r'0x[0-9a-fA-F]+', fn):
                if 'instr' not in positions:
                    raise ValueError('unresolved entry lacks instruction position')
                token = line.split()[positions.index('instr')]
                if not re.fullmatch(r'0x[0-9a-fA-F]+|[0-9]+', token):
                    raise ValueError('unresolved entry has compressed/relative instruction position')
                position = int(token, 16 if token.startswith('0x') else 10)
                anonymous = obj == '???'
                if '?' in obj and not anonymous:
                    raise ValueError('unsupported unresolved object identity')
                candidates = []
                for record in objects:
                    if record.get('pid') != pid:
                        raise ValueError('loader/profile PID mismatch')
                    segments = record.get('segments', [])
                    # Callgrind fn.c:new_obj_node assigns offset=0 to ???;
                    # dump.c:get_debug_pos emits runtime address minus offset.
                    address = position if anonymous else record['load_bias'] + position
                    if segments and any(s['flags'] & 1 and s['start'] <= address < s['end'] for s in segments):
                        path = record['object']
                        if path == '/proc/self/exe':
                            path = str(executable)
                        if not path or '?' in path:
                            continue
                        # Resolve actual filesystem aliases, preserving full object
                        # identities rather than merging equal basenames.
                        if not anonymous and obj != path:
                            try:
                                same_object = (Path(obj).is_absolute() and Path(path).is_absolute() and
                                               Path(obj).samefile(path))
                            except OSError:
                                continue
                            if not same_object:
                                continue
                            path = str(Path(path).resolve(strict=True))
                        base = min(s['start'] for s in segments)
                        candidates.append((path, address - base))
                if len(candidates) != 1:
                    raise ValueError('unresolved entry has missing/ambiguous loader mapping')
                path, offset = candidates[0]
                output.extend(['ob=' + path, 'fn=@offset:' + hex(offset), line,
                               'ob=' + obj, 'fn=' + fn])
                continue
        output.append(line)
    return '\n'.join(output) + '\n'


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


def profile(raw, library, executable, pid, documents=()):
    """Resolve and reconcile exclusive Callgrind instruction attribution.

    Validate profiler identity, resolve unknown sites through loader records,
    and require unique object/function rows summing to the summary. Return
    counts, raw text, and loader metadata; mark offset-resolved sites as
    unresolved. Invalid or ambiguous attribution raises ValueError.
    """
    identity = total_profile(raw, executable, pid)
    text = identity['raw']
    attributed = resolve_unmapped(text, documents, pid, executable)
    parsed = callgrind_costs(attributed, library, preserve_objects=True)
    summary = re.findall(r'^summary:\s*(\d+)\s*$', text, re.M)
    if len(summary) != 1 or int(summary[0]) != parsed['totals']['Ir']:
        raise ValueError('Callgrind summary does not reconcile with exclusive Ir')
    functions = parsed['functions']
    if len({(row['object'], row['function']) for row in functions}) != len(functions):
        raise ValueError('Callgrind object/function identity is ambiguous')
    if any(not row['object'] or '?' in row['object'] or not row['function'] or
           '?' in row['function'] or re.fullmatch(r'0x[0-9a-fA-F]+', row['function'])
           for row in functions):
        raise ValueError('Callgrind object/function resolution incomplete')
    for row in functions:
        if row['function'].startswith('@offset:'):
            row['unresolved'] = True
    return dict(total=int(summary[0]), functions=functions, raw=text, loader_data=[d for d in documents if d.get('startup') == 'loaded_object'],
                creator=re.search(r'^creator:[ \t]+([^\s]+)', text, re.M)[1], pid=pid)


def qualify(counts, profiles):
    """Qualify instruction-count ranges using same-bytes PR/control attribution.

    Require at least five counts per arm and one profiler identity, otherwise
    raise ValueError. A positive PR median, overlapping PR/control ranges,
    allowlisted variation, and constant unresolved sites yield RANGE-QUALIFIED;
    otherwise return INVALID with the offending sites and observed bounds.
    """
    if any(len(counts[arm]) < 5 for arm in ('base', 'pr', 'control')):
        raise ValueError('range qualification requires five fresh processes per arm')
    median = statistics.median(counts['pr'])
    if len({p['creator'] for samples in profiles.values() for p in samples}) != 1:
        raise ValueError('Callgrind profiler identity changed between samples')
    bounds = {arm: [min(values), max(values)] for arm, values in counts.items()}
    overlap = max(bounds[a][0] for a in ('pr', 'control')) <= min(bounds[a][1] for a in ('pr', 'control'))
    tables = [{(r['object'], r['function']): r['Ir'] for r in sample['functions']}
              for arm in ('pr', 'control') for sample in profiles[arm]]
    keys = set().union(*tables)
    varying = [dict(object=obj, function=fn, values=[t.get((obj, fn), 0) for t in tables])
               for obj, fn in sorted(keys) if len({t.get((obj, fn), 0) for t in tables}) > 1]
    unresolved_keys = {(r['object'], r['function']) for arm in ('pr', 'control')
                       for sample in profiles[arm] for r in sample['functions'] if r.get('unresolved')}
    constant = {key: tables[0][key] for key in unresolved_keys
                if all(key in t and t[key] == tables[0].get(key) for t in tables)}
    unresolved_invalid = sorted(unresolved_keys - constant.keys())
    outside = [row for row in varying if (row['object'], row['function']) not in unresolved_keys
               and not allowed(row['function'])]
    valid = median > 0 and overlap and not outside and not unresolved_invalid
    return dict(verdict='RANGE-QUALIFIED' if valid else 'INVALID', bounds=bounds,
                pr_median=median, overlap=overlap,
                base_minus_pr_bounds=[bounds['base'][0] - bounds['pr'][1],
                                      bounds['base'][1] - bounds['pr'][0]],
                varying_functions=varying, outside_allowlist=outside,
                constant_unresolved_count=len(constant), constant_unresolved_Ir=sum(constant.values()),
                invalid_unresolved=unresolved_invalid)
