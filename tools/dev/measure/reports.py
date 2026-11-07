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
