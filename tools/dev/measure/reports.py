"""Report assembly for the measurement recipes (#1290).

The perf-baseline, perf-matrix, perf-ci, perf-ratio and perf-decomp scripts own
their summaries and are run through bench.report. These are the two summaries
that were inline heredocs in the shell runners; they read a capture and return
the text the runner printed. Medians only: the confidence intervals and ratios
stay in perf-ci.py and perf-ratio.py, so there is one statistics engine.
Standard library only.
"""
import json
import statistics
from collections.abc import Callable


def _med(values) -> float:
    """The median, or NaN for no values."""
    return statistics.median(values) if values else float('nan')


def _jsonl(path) -> list[dict]:
    """The JSON object on each nonblank line of PATH."""
    with open(path, encoding='utf-8') as stream:
        return [json.loads(line) for line in map(str.strip, stream) if line]


def _by_backend(path, load: Callable[[dict], dict] = lambda row: row) -> dict[str, list[dict]]:
    """The capture's rows grouped by their `backend` label, in first-seen order."""
    rows: dict[str, list[dict]] = {}
    for row in _jsonl(path):
        rows.setdefault(row['backend'], []).append(load(row))
    return rows


def _flatten(row: dict) -> dict:
    """Lift the resident-set counters bisect nests into a row so every reader finds them.

    bisect nests them: ram-init rows carry them in after_init, ram-cycle rows peak
    in after_last (HWM is monotonic, so the last snapshot holds the process peak).
    """
    if 'rss_kib' not in row:
        snapshot = row.get('after_init' if row.get('mode') == 'ram-init' else 'after_last', {})
        row.update({key: snapshot[key] for key in ('rss_kib', 'hwm_kib') if key in snapshot})
    return row


def same_data_summary(speed, ram_init, ram_cycle) -> str:
    """The same-data matrix's per-cell medians: init, alloc, cold and steady ms, then RSS and HWM in KiB."""
    speed_rows = _by_backend(speed)
    init_rows = _by_backend(ram_init, _flatten)
    cycle_rows = _by_backend(ram_cycle, _flatten)

    def field(rows, key):
        return _med([row[key] for row in rows if key in row])
    lines = [f"{'cell':<18}{'init ms':>10}{'alloc ms':>10}{'cold ms':>10}{'steady ms':>11}"
             f"{'rss-init KiB':>14}{'hwm-init KiB':>14}{'rss-cycle KiB':>15}{'hwm-cycle KiB':>15}"]
    for label, rows in speed_rows.items():
        init = _med([row['init_ns'] for row in rows]) / 1e6
        alloc = _med([row['alloc_ns'] for row in rows]) / 1e6
        cold = _med([row['cycles_ns'][0] for row in rows]) / 1e6
        steady = _med([cycle for row in rows for cycle in row['cycles_ns'][1:]]) / 1e6
        before, after = init_rows.get(label, []), cycle_rows.get(label, [])
        lines.append(f"{label:<18}{init:>10.3f}{alloc:>10.3f}{cold:>10.3f}{steady:>11.3f}"
                     f"{field(before, 'rss_kib'):>14.0f}{field(before, 'hwm_kib'):>14.0f}"
                     f"{field(after, 'rss_kib'):>15.0f}{field(after, 'hwm_kib'):>15.0f}")
    return '\n'.join(lines) + '\n'


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
