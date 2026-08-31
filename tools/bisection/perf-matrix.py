#!/usr/bin/env python3
"""4-cell backend × implementation performance-matrix analysis.

Reads JSONL captures from bisect --perf (four backend labels) and
installed-prefix sizes, then produces a factorial-analysis report:
backend effects, implementation effects, and interaction.
"""

import argparse
import json
import math
import os
import pathlib
import statistics
import sys


def load_jsonl(path):
    rows = []
    with open(path, encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if line:
                rows.append(json.loads(line))
    return rows


def median(values):
    return statistics.median(values) if values else float("nan")


def cv_pct(values):
    if len(values) < 2:
        return float("nan")
    m = statistics.mean(values)
    return (statistics.stdev(values) / m * 100) if m else float("nan")


def ms(ns_values):
    return [v / 1_000_000.0 for v in ns_values]


def fmt_ms(ns_values):
    vals = ms(ns_values)
    return f"{median(vals):.3f} ms [{min(vals):.3f}, {max(vals):.3f}]"


def fmt_ms_short(ns_values):
    return f"{median(ms(ns_values)):.3f}"


def fmt_kib(values):
    return f"{median(values):,.0f}"


def ratio(a, b):
    ma, mb = median(a), median(b)
    if mb == 0 or math.isnan(mb):
        return float("nan")
    return ma / mb


def human(nbytes):
    return f"{nbytes / 1024 / 1024:.2f} MiB"


def group_by_backend(rows):
    g = {}
    for r in rows:
        g.setdefault(r["backend"], []).append(r)
    return g


# ── Speed extraction ─────────────────────────────────────────────────

def extract_speed(rows):
    return {
        "init": [r["init_ns"] for r in rows],
        "alloc": [r["alloc_ns"] for r in rows],
        "cold": [r["cycles_ns"][0] for r in rows],
        "steady": [c for r in rows for c in r["cycles_ns"][1:]],
    }


# ── RAM extraction ───────────────────────────────────────────────────

def extract_ram_init(rows):
    return {
        "rss": [r["after_init"]["rss_kib"] for r in rows],
        "hwm": [r["after_init"]["hwm_kib"] for r in rows],
        "anon": [r["after_init"]["rss_anon_kib"] for r in rows],
        "file": [r["after_init"]["rss_file_kib"] for r in rows],
    }


def extract_ram_cycle(rows):
    return {
        "hwm_first": [r["after_first"]["hwm_kib"] for r in rows],
        "hwm_last": [r["after_last"]["hwm_kib"] for r in rows],
    }


# ── Size measurement ─────────────────────────────────────────────────

def measure_size(prefix, data_dir):
    so_bytes = 0
    data_bytes = 0
    other_bytes = 0
    symlink_bytes = 0
    data_files = []
    prefix_p = pathlib.Path(prefix)
    if not prefix_p.exists():
        raise FileNotFoundError(f"size prefix does not exist: {prefix}")
    data_p = pathlib.Path(data_dir) if data_dir else None
    if data_p is not None and not data_p.exists():
        raise FileNotFoundError(f"data directory does not exist: {data_dir}")

    if data_p:
        for p in data_p.rglob("*"):
            if p.is_symlink():
                symlink_bytes += os.lstat(p).st_size
            elif p.is_file():
                sz = p.stat().st_size
                data_bytes += sz
                data_files.append((p.name, sz))

    for p in prefix_p.rglob("*"):
        if data_p and p.is_relative_to(data_p):
            continue
        if p.is_symlink():
            symlink_bytes += os.lstat(p).st_size
            continue
        if not p.is_file():
            continue
        sz = p.stat().st_size
        if ".so" in p.name:
            so_bytes += sz
        else:
            other_bytes += sz

    return {
        "so": so_bytes,
        "data": data_bytes,
        "other": other_bytes,
        "symlink_bytes": symlink_bytes,
        "total": so_bytes + data_bytes + other_bytes + symlink_bytes,
        "data_files": sorted(data_files, key=lambda x: -x[1]),
    }


# ── Report generation ────────────────────────────────────────────────

CELL_ORDER = [
    "libpinyin-tkrzw",
    "libpinyin-kc",
    "oxpinyin-kc",
    "oxpinyin-tkrzw",
]

CELL_MATRIX_ORDER = [
    "libpinyin-kc",
    "libpinyin-tkrzw",
    "oxpinyin-kc",
    "oxpinyin-tkrzw",
]


def report_speed(speed_groups):
    print("## Execution speed")
    print()
    print("Per-process, CPU-pinned. Each process: pinyin_init, pinyin_alloc_instance,")
    print("then N keystroke cycles (cycle 0 = cold, 1..N-1 = steady).")
    print()
    print("| Cell | init | alloc | cold cycle | steady cycle |")
    print("|---|---:|---:|---:|---:|")
    cells = {}
    for label in CELL_ORDER:
        s = extract_speed(speed_groups.get(label, []))
        cells[label] = s
        n = len(speed_groups.get(label, []))
        print(f"| {label} (n={n}) | {fmt_ms(s['init'])} | {fmt_ms(s['alloc'])} | "
              f"{fmt_ms(s['cold'])} | {fmt_ms(s['steady'])} |")
    print()
    print("### Variance (CV%)")
    print()
    print("| Cell | init CV% | steady CV% |")
    print("|---|---:|---:|")
    for label in CELL_ORDER:
        s = cells[label]
        print(f"| {label} | {cv_pct(ms(s['init'])):.1f}% | {cv_pct(ms(s['steady'])):.1f}% |")
    return cells


def report_ram(init_groups, cycle_groups):
    print()
    print("## RAM")
    print()
    print("Post-init values from init-only processes; cycle-peak from cycle processes.")
    print()
    print("| Cell | post-init RSS | post-init HWM | cycle-peak HWM |")
    print("|---|---:|---:|---:|")
    cells = {}
    for label in CELL_ORDER:
        ri = extract_ram_init(init_groups.get(label, []))
        rc = extract_ram_cycle(cycle_groups.get(label, []))
        cells[label] = {"init": ri, "cycle": rc}
        ni = len(init_groups.get(label, []))
        nc = len(cycle_groups.get(label, []))
        print(f"| {label} (init n={ni}, cycle n={nc}) | "
              f"{fmt_kib(ri['rss'])} KiB | {fmt_kib(ri['hwm'])} KiB | "
              f"{fmt_kib(rc['hwm_last'])} KiB |")
    print()
    print("### Memory composition (post-init)")
    print()
    print("| Cell | RssAnon | RssFile | RSS |")
    print("|---|---:|---:|---:|")
    for label in CELL_ORDER:
        ri = cells[label]["init"]
        print(f"| {label} | {fmt_kib(ri['anon'])} KiB | {fmt_kib(ri['file'])} KiB | "
              f"{fmt_kib(ri['rss'])} KiB |")
    return cells


def report_size(labels, size_prefixes, data_dirs):
    print()
    print("## Installed size (all .so stripped)")
    print()
    print("| Cell | .so | runtime data | total |")
    print("|---|---:|---:|---:|")
    cells = {}
    for label, prefix, ddir in zip(labels, size_prefixes, data_dirs):
        s = measure_size(prefix, ddir)
        cells[label] = s
        print(f"| {label} | {human(s['so'])} | {human(s['data'])} | {human(s['total'])} |")
    print()
    for label in labels:
        s = cells[label]
        if s["data_files"]:
            print(f"**{label}** data files:")
            print()
            for name, sz in s["data_files"]:
                print(f"- {name}: {human(sz)}")
            print()
    return cells


def _size_val(size_cells, label, key):
    cell = size_cells.get(label)
    if cell is None:
        return float("nan")
    v = cell.get(key)
    return float("nan") if v is None else v


def report_factorial(speed_cells, ram_cells, size_cells):
    print()
    print("## Factorial analysis")
    print()
    print("### 2×2 matrix (medians)")
    print()

    speed_metrics = [
        ("Init (ms)", lambda s: median(ms(s["init"]))),
        ("Cold cycle (ms)", lambda s: median(ms(s["cold"]))),
        ("Steady cycle (ms)", lambda s: median(ms(s["steady"]))),
    ]
    ram_metrics = [
        ("Post-init RSS (KiB)", lambda r: median(r["init"]["rss"])),
        ("Cycle-peak HWM (KiB)", lambda r: median(r["cycle"]["hwm_last"])),
    ]

    print("| Metric | libpinyin KC | libpinyin Tkrzw | oxpinyin KC | oxpinyin Tkrzw |")
    print("|---|---:|---:|---:|---:|")
    for name, fn in speed_metrics:
        vals = [fn(speed_cells[l]) for l in CELL_MATRIX_ORDER]
        print(f"| {name} | {vals[0]:.3f} | {vals[1]:.3f} | {vals[2]:.3f} | {vals[3]:.3f} |")
    for name, fn in ram_metrics:
        vals = [fn(ram_cells[l]) for l in CELL_MATRIX_ORDER]
        print(f"| {name} | {vals[0]:,.0f} | {vals[1]:,.0f} | {vals[2]:,.0f} | {vals[3]:,.0f} |")
    if size_cells:
        for name, key in [(".so (KiB)", "so"), ("Data (KiB)", "data"), ("Total (KiB)", "total")]:
            vals = [_size_val(size_cells, l, key) / 1024 for l in CELL_MATRIX_ORDER]
            print(f"| {name} | {vals[0]:,.0f} | {vals[1]:,.0f} | {vals[2]:,.0f} | {vals[3]:,.0f} |")

    print()
    print("### Backend effect (KC / Tkrzw ratio, holding implementation fixed)")
    print()
    print("| Metric | within libpinyin | within oxpinyin |")
    print("|---|---:|---:|")
    for name, fn in speed_metrics:
        lp = fn(speed_cells["libpinyin-kc"]) / fn(speed_cells["libpinyin-tkrzw"])
        ox = fn(speed_cells["oxpinyin-kc"]) / fn(speed_cells["oxpinyin-tkrzw"])
        print(f"| {name} | {lp:.3f}× | {ox:.3f}× |")
    for name, fn in ram_metrics:
        lp_kc = fn(ram_cells["libpinyin-kc"])
        lp_tk = fn(ram_cells["libpinyin-tkrzw"])
        ox_kc = fn(ram_cells["oxpinyin-kc"])
        ox_tk = fn(ram_cells["oxpinyin-tkrzw"])
        print(f"| {name} | {lp_kc/lp_tk:.3f}× | {ox_kc/ox_tk:.3f}× |")
    if size_cells:
        for name, key in [(".so", "so"), ("Data", "data"), ("Total", "total")]:
            lp_kc = _size_val(size_cells, "libpinyin-kc", key)
            lp_tk = _size_val(size_cells, "libpinyin-tkrzw", key)
            ox_kc = _size_val(size_cells, "oxpinyin-kc", key)
            ox_tk = _size_val(size_cells, "oxpinyin-tkrzw", key)
            lp_r = lp_kc / lp_tk if lp_tk else float("nan")
            ox_r = ox_kc / ox_tk if ox_tk else float("nan")
            print(f"| {name} | {lp_r:.3f}× | {ox_r:.3f}× |")

    print()
    print("### Implementation effect (oxpinyin / libpinyin ratio, holding backend fixed)")
    print()
    print("| Metric | with KC | with Tkrzw |")
    print("|---|---:|---:|")
    for name, fn in speed_metrics:
        kc = fn(speed_cells["oxpinyin-kc"]) / fn(speed_cells["libpinyin-kc"])
        tk = fn(speed_cells["oxpinyin-tkrzw"]) / fn(speed_cells["libpinyin-tkrzw"])
        print(f"| {name} | {kc:.3f}× | {tk:.3f}× |")
    for name, fn in ram_metrics:
        kc = fn(ram_cells["oxpinyin-kc"]) / fn(ram_cells["libpinyin-kc"])
        tk = fn(ram_cells["oxpinyin-tkrzw"]) / fn(ram_cells["libpinyin-tkrzw"])
        print(f"| {name} | {kc:.3f}× | {tk:.3f}× |")
    if size_cells:
        for name, key in [(".so", "so"), ("Data", "data"), ("Total", "total")]:
            ox_kc = _size_val(size_cells, "oxpinyin-kc", key)
            lp_kc = _size_val(size_cells, "libpinyin-kc", key)
            ox_tk = _size_val(size_cells, "oxpinyin-tkrzw", key)
            lp_tk = _size_val(size_cells, "libpinyin-tkrzw", key)
            kc_r = ox_kc / lp_kc if lp_kc else float("nan")
            tk_r = ox_tk / lp_tk if lp_tk else float("nan")
            print(f"| {name} | {kc_r:.3f}× | {tk_r:.3f}× |")

    print()
    print("### Interaction (ratio of ratios)")
    print()
    print("Values near 1.0 indicate no interaction; the two factors act independently.")
    print()
    print("| Metric | (oxpinyin KC/Tkrzw) / (libpinyin KC/Tkrzw) |")
    print("|---|---:|")
    for name, fn in speed_metrics:
        ox_ratio = fn(speed_cells["oxpinyin-kc"]) / fn(speed_cells["oxpinyin-tkrzw"])
        lp_ratio = fn(speed_cells["libpinyin-kc"]) / fn(speed_cells["libpinyin-tkrzw"])
        interaction = ox_ratio / lp_ratio if lp_ratio else float("nan")
        print(f"| {name} | {interaction:.3f} |")
    for name, fn in ram_metrics:
        ox_ratio = fn(ram_cells["oxpinyin-kc"]) / fn(ram_cells["oxpinyin-tkrzw"])
        lp_ratio = fn(ram_cells["libpinyin-kc"]) / fn(ram_cells["libpinyin-tkrzw"])
        interaction = ox_ratio / lp_ratio if lp_ratio else float("nan")
        print(f"| {name} | {interaction:.3f} |")
    if size_cells:
        for name, key in [(".so", "so"), ("Data", "data"), ("Total", "total")]:
            ox_kc = _size_val(size_cells, "oxpinyin-kc", key)
            ox_tk = _size_val(size_cells, "oxpinyin-tkrzw", key)
            lp_kc = _size_val(size_cells, "libpinyin-kc", key)
            lp_tk = _size_val(size_cells, "libpinyin-tkrzw", key)
            ox_r = ox_kc / ox_tk if ox_tk else float("nan")
            lp_r = lp_kc / lp_tk if lp_tk else float("nan")
            interaction = ox_r / lp_r if lp_r else float("nan")
            print(f"| {name} | {interaction:.3f} |")

    print()
    ox_kc_init = median(ms(speed_cells["oxpinyin-kc"]["init"]))
    lp_tk_init = median(ms(speed_cells["libpinyin-tkrzw"]["init"]))
    cross_ratio = ox_kc_init / lp_tk_init if lp_tk_init else float("nan")
    print(f"### Decomposition of the {cross_ratio:.1f}× init gap")
    print()
    print(f"Original cross-comparison (oxpinyin-KC / libpinyin-Tkrzw): {cross_ratio:.1f}×")
    print()
    lp_kc_init = median(ms(speed_cells["libpinyin-kc"]["init"]))
    ox_tk_init = median(ms(speed_cells["oxpinyin-tkrzw"]["init"]))
    print(f"- libpinyin KC init:  {lp_kc_init:.3f} ms")
    print(f"- libpinyin Tkrzw init: {lp_tk_init:.3f} ms")
    print(f"- oxpinyin KC init:   {ox_kc_init:.3f} ms")
    print(f"- oxpinyin Tkrzw init: {ox_tk_init:.3f} ms")
    print()
    be_lp = lp_kc_init / lp_tk_init if lp_tk_init else float("nan")
    be_ox = ox_kc_init / ox_tk_init if ox_tk_init else float("nan")
    ie_kc = ox_kc_init / lp_kc_init if lp_kc_init else float("nan")
    ie_tk = ox_tk_init / lp_tk_init if lp_tk_init else float("nan")
    print(f"Backend effect (KC/Tkrzw):  libpinyin {be_lp:.1f}×, oxpinyin {be_ox:.1f}×")
    print(f"Implementation effect (ox/lp): with KC {ie_kc:.1f}×, with Tkrzw {ie_tk:.1f}×")


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    s = sub.add_parser("summarize")
    s.add_argument("--speed", required=True)
    s.add_argument("--ram-init", required=True)
    s.add_argument("--ram-cycle", required=True)
    s.add_argument("--labels", nargs="+", required=True)
    s.add_argument("--size-prefixes", nargs="+", required=True)
    s.add_argument("--data-dirs", nargs="+", required=True)
    args = parser.parse_args(argv)

    speed_rows = load_jsonl(args.speed)
    init_rows = load_jsonl(args.ram_init)
    cycle_rows = load_jsonl(args.ram_cycle)

    speed_groups = group_by_backend(speed_rows)
    init_groups = group_by_backend(init_rows)
    cycle_groups = group_by_backend(cycle_rows)

    print("# Performance Matrix — Backend × Implementation")
    print()

    speed_cells = report_speed(speed_groups)
    ram_cells = report_ram(init_groups, cycle_groups)
    size_cells = report_size(args.labels, args.size_prefixes, args.data_dirs)
    report_factorial(speed_cells, ram_cells, size_cells)

    return 0


def _test_sibling_directory_not_excluded():
    """Regression: a sibling directory whose name shares a prefix with
    data_p (e.g. data-old vs data) must not be excluded from the prefix
    walk."""
    import tempfile
    with tempfile.TemporaryDirectory() as tmp:
        prefix = pathlib.Path(tmp)
        data = prefix / "data"
        sibling = prefix / "data-old"
        data.mkdir()
        sibling.mkdir()
        (data / "table.kct").write_bytes(b"x" * 100)
        (sibling / "leftover.kct").write_bytes(b"x" * 50)

        result = measure_size(str(prefix), str(data))
        assert result["data"] == 100, f"data should be 100, got {result['data']}"
        assert result["other"] == 50, (
            f"sibling file should count as other (50), got {result['other']}"
        )


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--test":
        _test_sibling_directory_not_excluded()
        print("ok")
        sys.exit(0)
    sys.exit(main(sys.argv[1:]))
