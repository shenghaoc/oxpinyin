#!/usr/bin/env python3
"""Summarize oxpinyin-vs-oracle performance-baseline captures and sizes.

The capture JSONL is produced by `bisect --perf` (see run-perf-baseline.sh).
Sizes are measured from the installed prefixes, not from cargo target dirs.
"""

import argparse
import json
import os
import pathlib
import statistics
import sys


def load_jsonl(path):
    rows = []
    with open(path, encoding="utf-8") as stream:
        for line in stream:
            line = line.strip()
            if line:
                rows.append(json.loads(line))
    return rows


def median(values):
    return statistics.median(values) if values else float("nan")


def ms(values_ns):
    return [value / 1_000_000.0 for value in values_ns]


def fmt_ms(values_ns):
    vals = ms(values_ns)
    return f"{median(vals):.3f} ms [{min(vals):.3f}, {max(vals):.3f}]"


def fmt_kib(values):
    return f"{median(values):.0f} KiB [{min(values):.0f}, {max(values):.0f}]"


def ratio(oxp, oracle):
    return median(oxp) / median(oracle) if median(oracle) else float("nan")


def summarize_speed(rows):
    print("## Execution speed — C-ABI to C-ABI (per process, CPU-pinned)")
    print()
    print("Each process reports one `pinyin_init`, one `pinyin_alloc_instance`,")
    print("then N 20-input keystroke cycles. Cycle 0 is cold; cycles 1..N-1")
    print("are steady state. Times are medians [min, max].")
    print()
    print("| Backend | init | alloc | cycle 0 (cold) | cycles 1..N (steady) |")
    print("|---|---:|---:|---:|---:|")
    groups = {}
    for row in rows:
        groups.setdefault(row["backend"], []).append(row)
    for backend in ("oracle", "oxpinyin"):
        group = groups[backend]
        init = [r["init_ns"] for r in group]
        alloc = [r["alloc_ns"] for r in group]
        cold = [r["cycles_ns"][0] for r in group]
        steady = [c for r in group for c in r["cycles_ns"][1:]]
        print(f"| {backend} (n={len(group)}) | {fmt_ms(init)} | {fmt_ms(alloc)} | "
              f"{fmt_ms(cold)} | {fmt_ms(steady)} |")
        groups[backend] = {
            "init": init,
            "alloc": alloc,
            "cold": cold,
            "steady": steady,
        }
    print()
    for metric, label in (
        ("init", "init"),
        ("alloc", "alloc"),
        ("cold", "cold cycle"),
        ("steady", "steady cycle"),
    ):
        print(f"- {label} oxpinyin/oracle ratio: "
              f"{ratio(groups['oxpinyin'][metric], groups['oracle'][metric]):.3f}x")


def summarize_ram(init_rows, cycle_rows):
    print("## RAM — /proc/self/status (same harness, both backends)")
    print()
    print("VmHWM is monotonic per process, so after-last HWM is the lifetime")
    print("peak, not a cycle-only reset. Post-init rows come from ram-init")
    print("processes; cycle rows come from ram-cycle processes.")
    print()
    print("| Backend | post-init RSS | post-init HWM | after-first HWM | after-last HWM (peak) |")
    print("|---|---:|---:|---:|---:|")
    for backend in ("oracle", "oxpinyin"):
        init_group = [r for r in init_rows if r["backend"] == backend]
        cycle_group = [r for r in cycle_rows if r["backend"] == backend]
        init_rss = [r["after_init"]["rss_kib"] for r in init_group]
        init_hwm = [r["after_init"]["hwm_kib"] for r in init_group]
        first_hwm = [r["after_first"]["hwm_kib"] for r in cycle_group]
        last_hwm = [r["after_last"]["hwm_kib"] for r in cycle_group]
        print(f"| {backend} (init n={len(init_group)}, cycle n={len(cycle_group)}) | "
              f"{fmt_kib(init_rss)} | {fmt_kib(init_hwm)} | "
              f"{fmt_kib(first_hwm)} | {fmt_kib(last_hwm)} |")
    print()
    for backend in ("oracle", "oxpinyin"):
        cycle_group = [r for r in cycle_rows if r["backend"] == backend]
        init_group = [r for r in init_rows if r["backend"] == backend]
        print(f"- {backend}: post-init RSS {fmt_kib([r['after_init']['rss_kib'] for r in init_group])}; "
              f"post-init RssAnon {fmt_kib([r['after_init']['rss_anon_kib'] for r in init_group])}; "
              f"post-init RssFile {fmt_kib([r['after_init']['rss_file_kib'] for r in init_group])}; "
              f"cycle-peak HWM {fmt_kib([r['after_last']['hwm_kib'] for r in cycle_group])}.")
    print()
    print("- Ratio notes: oxpinyin/oracle post-init RSS "
          f"{ratio([r['after_init']['rss_kib'] for r in init_rows if r['backend'] == 'oxpinyin'], [r['after_init']['rss_kib'] for r in init_rows if r['backend'] == 'oracle']):.2f}x; "
          "peak HWM "
          f"{ratio([r['after_last']['hwm_kib'] for r in cycle_rows if r['backend'] == 'oxpinyin'], [r['after_last']['hwm_kib'] for r in cycle_rows if r['backend'] == 'oracle']):.2f}x.")


def classify_size(prefix, backend):
    regular = []
    symlink_bytes = 0
    for path in pathlib.Path(prefix).rglob("*"):
        if path.is_symlink():
            symlink_bytes += os.lstat(path).st_size
        elif path.is_file():
            regular.append((path, path.stat().st_size))
    code = []
    data = []
    other = []
    for path, size in regular:
        rel = path.relative_to(prefix).as_posix()
        if backend == "oxpinyin":
            if rel.startswith("share/oxpinyin/"):
                data.append((path, size))
            elif path.name == "libpinyin_capi.so.0.1.0" or path.name.endswith(".a"):
                code.append((path, size))
            else:
                other.append((path, size))
        else:
            if "lib/libpinyin/data" in rel or rel.startswith("lib/libpinyin/data"):
                data.append((path, size))
            elif rel.startswith("lib/") and (".so" in path.name or path.name.endswith(".a")):
                code.append((path, size))
            elif "/bin/" in rel:
                code.append((path, size))
            else:
                other.append((path, size))
    shared = [size for path, size in code if ".so" in path.name]
    return {
        "regular_files": regular,
        "code": code,
        "data": data,
        "other": other,
        "symlink_bytes": symlink_bytes,
        "shared_bytes": sum(shared),
        "code_bytes": sum(size for _, size in code),
        "data_bytes": sum(size for _, size in data),
        "other_bytes": sum(size for _, size in other),
        "total_bytes": sum(size for _, size in regular) + symlink_bytes,
        "runtime_bytes": sum(shared) + sum(size for _, size in data),
    }


def human(nbytes):
    return f"{nbytes:,} bytes ({nbytes / 1024 / 1024:.2f} MiB)"


def summarize_size(oracle_prefix, oxpinyin_prefix):
    oracle = classify_size(oracle_prefix, "oracle")
    oxpinyin = classify_size(oxpinyin_prefix, "oxpinyin")
    print("## Size on disk — installed prefixes only")
    print()
    print("| Side | shared object | runtime data | runtime footprint | total install |")
    print("|---|---:|---:|---:|---:|")
    print(f"| oracle | {human(oracle['shared_bytes'])} | {human(oracle['data_bytes'])} | "
          f"{human(oracle['runtime_bytes'])} | {human(oracle['total_bytes'])} |")
    print(f"| oxpinyin | {human(oxpinyin['shared_bytes'])} | {human(oxpinyin['data_bytes'])} | "
          f"{human(oxpinyin['runtime_bytes'])} | {human(oxpinyin['total_bytes'])} |")
    print()
    print("Code (all .so/.a/executables in prefix) vs data:")
    print()
    print("| Side | installed code | installed data | other files |")
    print("|---|---:|---:|---:|")
    print(f"| oracle | {human(oracle['code_bytes'])} | {human(oracle['data_bytes'])} | "
          f"{human(oracle['other_bytes'])} |")
    print(f"| oxpinyin | {human(oxpinyin['code_bytes'])} | {human(oxpinyin['data_bytes'])} | "
          f"{human(oxpinyin['other_bytes'])} |")
    print()
    print(f"- Shared-object ratio oxpinyin/oracle: "
          f"{oxpinyin['shared_bytes'] / oracle['shared_bytes']:.3f}x")
    print(f"- Runtime-data ratio oxpinyin/oracle: "
          f"{oxpinyin['data_bytes'] / oracle['data_bytes']:.3f}x")
    print(f"- Runtime-footprint ratio oxpinyin/oracle: "
          f"{oxpinyin['runtime_bytes'] / oracle['runtime_bytes']:.3f}x")
    print(f"- Total-install ratio oxpinyin/oracle: "
          f"{oxpinyin['total_bytes'] / oracle['total_bytes']:.3f}x")
    print()
    print("oxpinyin runtime data:")
    for path, size in sorted(oxpinyin["data"], key=lambda item: -item[1]):
        print(f"  {path.relative_to(oxpinyin_prefix)}: {human(size)}")
    print("oracle runtime data:")
    for path, size in sorted(oracle["data"], key=lambda item: -item[1])[:8]:
        print(f"  {path.relative_to(oracle_prefix)}: {human(size)}")


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    speed = sub.add_parser("summarize-speed")
    speed.add_argument("jsonl")
    ram = sub.add_parser("summarize-ram")
    ram.add_argument("ram_init_jsonl")
    ram.add_argument("ram_cycle_jsonl")
    size = sub.add_parser("size")
    size.add_argument("oracle_prefix")
    size.add_argument("oxpinyin_prefix")
    args = parser.parse_args(argv)

    if args.command == "summarize-speed":
        summarize_speed(load_jsonl(args.jsonl))
    elif args.command == "summarize-ram":
        summarize_ram(load_jsonl(args.ram_init_jsonl), load_jsonl(args.ram_cycle_jsonl))
    elif args.command == "size":
        summarize_size(args.oracle_prefix, args.oxpinyin_prefix)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
