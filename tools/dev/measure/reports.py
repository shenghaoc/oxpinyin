"""Report assembly for the callgrind differential (#1290).

perf-ci.py owns its summary and runs through bench.report. This is the summary
that was an inline heredoc in the shell runner; it reads a capture and returns
the text the runner printed. Medians only: the confidence intervals stay in
perf-ci.py, so there is one statistics engine. Standard library only.
"""
import json
import statistics


def _jsonl(path) -> list[dict]:
    """The JSON object on each nonblank line of PATH."""
    with open(path, encoding='utf-8') as stream:
        return [json.loads(line) for line in map(str.strip, stream) if line]


def hardware_instructions(path) -> str:
    """Median hardware instructions per steady cycle for each cell, and the ratio of two cells.

    A run that reported no counter (a host without a PMU, or a container denied
    perf_event_open) turns the whole summary into the NO HARDWARE COUNTERS note:
    the capture goes on without it, with the distortion left unquantified.
    """
    runs: dict[str, list[tuple]] = {}
    hw_errno = None
    for row in _jsonl(path):
        if 'steady_hw_instructions' not in row:
            hw_errno = row.get('hw_errno', 'absent')
            continue
        runs.setdefault(row.get('backend', '?'), []).append(
            (row['steady_hw_instructions'], row.get('steady_hw_cycles', 0)))
    if hw_errno is not None:
        return '\n'.join([
            f'NO HARDWARE COUNTERS: a run reported none (hw_errno={hw_errno}).',
            'On a host with no PMU at all (Apple-silicon containers, some',
            'microVMs) this is expected and permanent, and the Ir differential',
            'above still stands on its own. On a Linux host that does have a',
            'PMU it means the container denied perf_event_open: rerun with',
            '  --security-opt label=disable --security-opt seccomp=unconfined',
            'Distortion remains UNQUANTIFIED either way, so no stall verdict',
            'may rest on callgrind Ir alone. Continuing with the remaining',
            'stages rather than aborting the capture.']) + '\n'
    lines, medians = [], {}
    for backend, values in sorted(runs.items()):
        counts = [value[0] for value in values]
        cycles = {value[1] for value in values}
        medians[backend] = statistics.median(counts)
        per_cycle = medians[backend] / max(cycles or {1})
        lines.append(f'{backend}: n={len(counts)} steady_cycles={sorted(cycles)} '
                     f'median={medians[backend]:.0f} per-cycle={per_cycle:,.0f}')
    labels = sorted(medians)
    if len(labels) == 2:
        ratio = medians[labels[1]] / medians[labels[0]]
        lines += [f'hardware instruction ratio ({labels[1]} / {labels[0]}): {ratio:.4f}',
                  'Compare against the callgrind Ir ratio over the same region;',
                  'the difference between the two ratios is the measured',
                  'malloc-interception distortion.']
    return '\n'.join(lines) + '\n' if lines else ''


# Report order: the instruction count and wall time lead. peak_rss_bytes stays last;
# a profile launch has no direct-child reading of it.
MEASURES = ('loop_instructions', 'wall_seconds', 'loop_cpu_cycles', 'cpu_seconds',
            'loop_runnable_seconds', 'lifetime_max_phys_footprint_bytes', 'peak_rss_bytes')
UNITS = {'loop_instructions': 'instructions retired in the measured loop (ri_instructions delta)',
         'wall_seconds': 'seconds, measured loop',
         'loop_cpu_cycles': 'CPU cycles in the measured loop (ri_cycles delta)',
         'cpu_seconds': 'seconds, measured loop',
         'loop_runnable_seconds': 'seconds on a core or waiting for one in the measured loop'
                                  ' (ri_runnable_time delta, advanced at scheduling events)',
         'lifetime_max_phys_footprint_bytes': 'bytes, process lifetime peak',
         'peak_rss_bytes': 'bytes, process lifetime peak'}
COUNTS = ('loop_instructions', 'loop_cpu_cycles')

# Windows report order: the cycle count and wall time lead; commit is the memory figure.
WINDOWS_MEASURES = ('loop_cpu_cycles', 'wall_seconds', 'cpu_seconds',
                    'peak_private_commit_bytes', 'peak_working_set_bytes')
WINDOWS_UNITS = {'loop_cpu_cycles': 'CPU cycles charged to the process, all threads, in the measured loop'
                                    ' (QueryProcessCycleTime delta); a count, not convertible to seconds',
                 'wall_seconds': 'seconds, measured loop (QueryPerformanceCounter)',
                 'cpu_seconds': 'seconds, measured loop (GetProcessTimes user+kernel, all threads);'
                                ' secondary, 1/64-second resolution',
                 'peak_private_commit_bytes': 'bytes, process lifetime peak (PeakPagefileUsage)',
                 'peak_working_set_bytes': 'bytes, process lifetime peak; diagnostic'}
BELOW_RESOLUTION = ('a sample read 0: below the 1/64-second GetProcessTimes resolution;'
                    ' no B/A ratio this session')


def observation(raw, *, cycles, operations, fingerprint=None, child_rss=True, windows=False):
    """Validate one complete driver result; an unread required metric is an error."""
    import math
    import re
    row = json.loads(raw)
    if (row.get('cycles') != cycles or row.get('operations') != operations
            or not isinstance(row.get('pid'), int) or row['pid'] <= 0):
        raise ValueError('incomplete workload')
    if not re.fullmatch('[0-9a-f]{16}', str(row.get('fingerprint', ''))):
        raise ValueError('missing operation fingerprint')
    if fingerprint is not None and row['fingerprint'] != fingerprint:
        raise ValueError('operation fingerprint mismatch')
    measures = WINDOWS_MEASURES if windows else (MEASURES if child_rss else MEASURES[:-1])
    for name in measures:
        value = row.get(name)
        # The kernel advances runnable time at scheduling events only; a short loop reads 0.
        # GetProcessTimes advances in 1/64-second ticks; a zero only drops that metric's ratio.
        zero_allowed = name == 'loop_runnable_seconds' or (windows and name == 'cpu_seconds')
        if (isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value)
                or value < 0 or (value == 0 and not zero_allowed)):
            raise ValueError('broken required capture: ' + name)
        if (name.endswith('_bytes') or name in COUNTS) and not isinstance(value, int):
            raise ValueError('count must be an integer: ' + name)
    return row


def comparison(rows, *, windows=False):
    """Paired block estimates; controls are descriptive and never produce a verdict.

    Runnable time is context for wall time and carries no B/A ratio: each arm
    reports it over its CPU time, and an excess is time spent waiting for a core.
    Windows byte metrics are paired B minus A differences, not ratios.
    One zero GetProcessTimes reading drops the cpu_seconds ratio for the whole
    session, comparison and control alike; every other metric is unaffected.
    """
    from . import stats
    unresolved = windows and any(not row['cpu_seconds'] for row in rows)
    result = {'units': WINDOWS_UNITS if windows else UNITS}
    for kind in ('comparison', 'control'):
        blocks = {}
        for row in rows:
            if row['kind'] == kind:
                blocks.setdefault(row['block'], {})[row['arm']] = row
        pairs = [block for block in blocks.values() if set(block) == {'A', 'B'}]
        if not pairs:
            continue
        def values(arm, metric):
            return [pair[arm][metric] for pair in pairs]
        result[kind] = {}
        for metric in (WINDOWS_MEASURES if windows else MEASURES):
            entry = {arm: {**stats.summary(values(arm, metric)), **stats.paired_bootstrap(values(arm, metric))}
                     for arm in 'AB'}
            if metric == 'loop_runnable_seconds':
                for arm in 'AB':
                    entry[arm]['over_cpu_seconds'] = stats.paired_ratio(values(arm, metric),
                                                                        values(arm, 'cpu_seconds'))
            elif unresolved and metric == 'cpu_seconds':
                entry['no_ratio'] = BELOW_RESOLUTION
            elif windows and metric.endswith('_bytes'):
                entry['B_minus_A_bytes'] = stats.paired_difference(values('B', metric), values('A', metric))
            else:
                entry['B_over_A'] = stats.paired_ratio(values('B', metric), values('A', metric))
            result[kind][metric] = entry
    return result


def capture_rows(xml, *, pid, uuid, allocations):
    """Require target-process data and a product frame in its recorded backtrace.

    xctrace XML interns processes, threads, frames and binaries using id/ref.
    Resolve those references before attributing any row, never by searching the
    whole export for an unrelated process or symbol.
    """
    import re
    import xml.etree.ElementTree as ET
    root = ET.fromstring(xml)
    # A combined export references objects defined in earlier tables.
    ids = {node.attrib['id']: node for node in root.iter() if 'id' in node.attrib}

    def expand(node, seen=frozenset()):
        ref = node.get('ref')
        if ref:
            if ref in seen or ref not in ids:
                return []
            return expand(ids[ref], seen | {ref})
        return [node, *(child for item in node for child in expand(item, seen))]

    count = product = 0
    for row in root.iter('row'):
        nodes = expand(row)
        processes = [node for node in nodes if node.tag == 'process']
        if not any(any(child.tag == 'pid' and child.text == str(pid) for child in expand(process))
                   or re.search(r'\(' + str(pid) + r'\)$', process.get('fmt', '')) for process in processes):
            continue
        def numeric(tags, minimum):
            return any(node.tag in tags and node.text is not None
                       and re.fullmatch(r'[0-9]+', node.text.strip())
                       and int(node.text) >= minimum for node in nodes)
        if not numeric({'sample-time', 'start-time', 'timestamp', 'time'}, 0):
            continue
        if allocations and not numeric({'size', 'allocation-size', 'alloc-size'}, 1):
            continue
        stacks = [node for node in nodes if node.tag in ('backtrace', 'tagged-backtrace')]
        if not stacks:
            continue
        count += 1
        for stack in stacks:
            for frame in (node for node in expand(stack) if node.tag == 'frame'):
                name = frame.get('name', frame.get('fmt', ''))
                binaries = [node for node in expand(frame) if node.tag == 'binary']
                known = (name.lstrip('_') in {'pinyin_init', 'pinyin_guess_candidates',
                                             'pinyin_parse_more_full_pinyins'}
                         or (name.startswith('_R') and '16oxpinyin_runtime' in name
                             and name.endswith('11RuntimeDict11fill_lookup')))
                if (known and any(
                        binary.get('UUID', binary.get('uuid', '')).replace('-', '').lower() == uuid for binary in binaries)):
                    product += 1
    if not count or not product:
        raise ValueError('capture lacks target-process data or a resolved product backtrace')
    return {'usable_rows': count, 'product_frames': product}


def driver_result(text):
    """xctrace can merge an unterminated product diagnostic before the final JSON."""
    import re
    matches = re.findall(r'(?m)\{[^\n]*\}\s*\Z', text)
    if len(matches) != 1:
        raise ValueError('missing final driver result')
    return json.loads(matches[0])


def allocation_path(toc, *, pid, driver, detail_name="Allocations List"):
    """Select the listed detail only for the single, successfully exited driver."""
    from pathlib import Path
    import xml.etree.ElementTree as ET
    root = ET.fromstring(toc)
    run = root.find("run[@number='1']")
    if run is None:
        raise ValueError('missing Allocations run')
    target = run.find('info/target/process')
    processes = [node for node in run.findall('processes/process') if node.get('pid') != '0']
    if (target is None or target.get('pid') != str(pid) or target.get('name') != 'bisect'
            or target.get('type') != 'launched' or target.get('return-exit-status') != '0'
            or len(processes) != 1 or processes[0].get('pid') != str(pid)
            or Path(processes[0].get('path', '')).resolve() != Path(driver).resolve()):
        raise ValueError('Allocations target is not the completed driver process')
    if detail_name not in ("Allocations List", "Statistics"):
        raise ValueError("unknown allocation detail")
    detail = run.find(f"tracks/track[@name='Allocations']/details/detail[@name='{detail_name}']")
    if detail is None or detail.get('kind') != 'table':
        raise ValueError(f'TOC does not expose {detail_name}')
    return f'/trace-toc/run[@number="1"]/tracks/track[@name="Allocations"]/details/detail[@name="{detail_name}"]'


def allocation_rows(xml, *, uuid, dylib):
    """Validate per-allocation detail attributes and symbolize its product backtraces.

    This detail has no per-row PID. The caller must first validate its single
    launched target with allocation_path. Its default view is allocations outstanding at process exit;
    report that scope, never call it the complete allocation-event history.
    """
    from pathlib import Path
    import re
    import xml.etree.ElementTree as ET
    from . import bench, identity
    dsym = Path(str(dylib) + '.dSYM')
    if identity.macho_uuid(dylib) != uuid or identity.macho_uuid(dsym) != uuid:
        raise ValueError('allocation symbols do not match measured UUID')
    dwarf = dsym / 'Contents/Resources/DWARF' / Path(dylib).name
    if not dwarf.is_file():
        raise ValueError('missing measured DWARF')
    root = ET.fromstring(xml)
    ids = {node.get('id'): node for node in root.iter() if 'id' in node.attrib}
    def resolve(node):
        if node is None:
            raise ValueError('allocation frame has no binary')
        if 'ref' in node.attrib:
            if node.get('ref') not in ids:
                raise ValueError('unresolved allocation backtrace reference')
            return ids[node.get('ref')]
        return node
    rows = list(root.iter('row'))
    product_rows, addresses = [], {}
    live = 0
    for row in rows:
        if (not re.fullmatch(r'0x[0-9a-fA-F]+', row.get('address', ''))
                or not re.fullmatch(r'[1-9][0-9]*', row.get('size', ''))
                or not row.get('category') or row.get('live') not in ('true', 'false')
                or not re.fullmatch(r'[0-9]+:[0-5][0-9]\.[0-9]{3}\.[0-9]{3}', row.get('timestamp', ''))):
            raise ValueError('invalid allocation address/size/category/time/live fields')
        live += row.get('live') == 'true'
        stack = row.find('backtrace')
        if stack is None or not list(stack):
            raise ValueError('allocation row has no backtrace')
        locations = []
        for reference in stack:
            frame = resolve(reference)
            if frame.tag != 'frame':
                raise ValueError('invalid allocation backtrace frame')
            binary = frame.find('binary')
            if binary is None:
                continue  # e.g. the explicit "Call stack limit reached" sentinel.
            binary = resolve(binary)
            if binary.get('UUID', '').replace('-', '').lower() != uuid:
                continue
            base, address = binary.get('load-addr', ''), frame.get('addr', '')
            if not all(re.fullmatch(r'0x[0-9a-fA-F]+', value) for value in (base, address)):
                raise ValueError('invalid product backtrace address')
            addresses.setdefault(base, set()).add(address)
            locations.append((base, address))
        product_rows.append(locations)
    resolved = {}
    for base, values in addresses.items():
        ordered = sorted(values, key=lambda value: int(value, 16))
        done = bench.capture(['xcrun', 'atos', '-arch', 'arm64', '-o', dwarf, '-l', base, *ordered])
        names = done.stdout.decode().splitlines()
        if done.status or len(names) != len(ordered):
            raise ValueError('allocation backtrace symbolication failed')
        resolved.update(((base, address), name.split(' (in ', 1)[0]) for address, name in zip(ordered, names))
    known = {'pinyin_init', 'pinyin_guess_candidates', 'pinyin_parse_more_full_pinyins'}
    functions = sorted(set(resolved.values()) & known)
    attributed = sum(any(resolved.get(location) in known for location in locations) for locations in product_rows)
    if not rows or not functions or not attributed:
        raise ValueError('Allocations List lacks a resolved known product backtrace')
    return dict(allocation_rows=len(rows), live_rows=live, product_rows=attributed,
                resolved_product_functions=functions, scope='allocations outstanding at process exit')


def allocation_statistics(xml):
    """Validate overall and category statistics for the TOC-validated driver."""
    import re
    import xml.etree.ElementTree as ET
    fields = ('count-total', 'count-transient', 'count-persistent',
              'total-bytes', 'transient-bytes', 'persistent-bytes')
    categories = {}
    for row in ET.fromstring(xml).iter('row'):
        category = row.get('category')
        if not category or category in categories or any(
                not re.fullmatch(r'[0-9]+', row.get(field, '')) for field in fields):
            raise ValueError('invalid allocation statistics category/count/bytes')
        values = {field: int(row.get(field)) for field in fields}
        if (values['count-total'] != values['count-transient'] + values['count-persistent']
                or values['total-bytes'] != values['transient-bytes'] + values['persistent-bytes']):
            raise ValueError('inconsistent allocation statistics totals')
        categories[category] = values
    overall = categories.get('All Heap & Anonymous VM')
    if overall is None or not any(not name.startswith('All ') for name in categories):
        raise ValueError('missing overall or per-category allocation statistics')
    return dict(overall=overall, categories=categories)
