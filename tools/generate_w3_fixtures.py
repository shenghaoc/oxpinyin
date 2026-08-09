#!/usr/bin/env python3
"""Generate mini fixture tables for W3 loader development.

Reads the pinned oracle's installed content files and truncates each to
a small number of records, producing portable test fixtures under
fixtures/w3/.  Run twice, identical output (reproducible).

Usage:
  python3 tools/generate_w3_fixtures.py [--data-dir PATH] [--max-records N]
"""

import argparse
import hashlib
import os
import struct
import sys
from pathlib import Path

# ── record size helpers ─────────────────────────────────────────────

def sub_record_size(ng: int, fl: int) -> int:
    """Size of a simple sub-record (fl <= 2)."""
    if fl == 0:
        return 6 + ng * 8
    if fl == 1:
        if ng == 1:
            return 14
        return 6 + 8 + (ng - 1) * 6 + 2
    if fl == 2:
        return 6 + (ng + 1) * 8
    return 0


def record_size(ng: int, fl: int, data: bytes, pos: int) -> int:
    """Compute size of a record at *pos* in *data*."""
    if fl <= 2:
        return sub_record_size(ng, fl)

    # fl >= 3 — extra token pairs + optional padding
    extra = fl - 1
    eff = ng + extra

    if ng < 10:
        base = 6 + eff * 8
        if ng >= 3:
            base += (ng - 2) * 2
        if fl == 4 and ng >= 5:
            base += 2
        return base

    # fl=3, ng >= 10: parse nested fl=1 sub-records
    inner = pos + 6
    bound = min(pos + 6 + eff * 30, len(data))
    while inner < bound:
        if inner + 2 > len(data):
            break
        s_ng = data[inner]
        s_fl = data[inner + 1]
        if s_ng == 0 or s_ng > 30 or s_fl not in (0, 1, 2):
            break
        sz = sub_record_size(s_ng, s_fl)
        if sz == 0 or inner + sz > bound:
            break
        inner += sz
    return max(inner - pos, 8)


def parse_records(data: bytes, data_start: int, nitems: int) -> list[tuple[int, int]]:
    """Return list of (offset, size) for each record."""
    records = []
    pos = data_start
    while pos < len(data) and len(records) < nitems:
        if pos + 2 > len(data):
            break
        ng = data[pos]
        fl = data[pos + 1]
        if ng == 0 or ng > 30:
            break
        sz = record_size(ng, fl, data, pos)
        if pos + sz > len(data):
            break
        records.append((pos, sz))
        pos += sz
    return records


# ── header helpers ──────────────────────────────────────────────────

HEADER_FMT = "<7I"  # 28 bytes: 7 × u32 LE
HEADER_SIZE = 28
PRIMARY_IDX_SIZE = 35 * 4  # 140 bytes


def read_header(data: bytes) -> dict:
    fields = struct.unpack_from(HEADER_FMT, data, 0)
    return {
        "data_size": fields[0],
        "magic": fields[1],
        "nitems": fields[2],
        "version": fields[3],
        "capacity": fields[4],
        "data_size2": fields[5],
        "ngroups": fields[6],
    }


def write_header(hdr: dict) -> bytes:
    return struct.pack(
        HEADER_FMT,
        hdr["data_size"],
        hdr["magic"],
        hdr["nitems"],
        hdr["version"],
        hdr["capacity"],
        hdr["data_size"],  # duplicate
        hdr["ngroups"],
    )


def data_start_offset(nitems: int, ngroups: int = 35) -> int:
    """Compute byte offset where the data section starts."""
    sec_entries = max(0, nitems - ngroups)
    preamble = 10 if sec_entries > 0 else 6
    return HEADER_SIZE + PRIMARY_IDX_SIZE + sec_entries * 4 + preamble


# ── main ────────────────────────────────────────────────────────────

ADDON_FILES = [
    "art.bin", "culture.bin", "economy.bin", "geology.bin",
    "history.bin", "life.bin", "nature.bin", "people.bin",
    "science.bin", "society.bin", "sport.bin", "technology.bin",
]

SYSTEM_FILES = ["gb_char.bin", "gbk_char.bin", "opengram.bin", "merged.bin"]


def build_header(hdr: dict) -> bytes:
    """Build 28-byte header from dict."""
    return struct.pack(
        HEADER_FMT,
        hdr["data_size"],
        hdr["magic"],
        hdr["nitems"],
        hdr["version"],
        hdr["capacity"],
        hdr["data_size"],
        hdr["ngroups"],
    )


def build_indices(primary: bytes, secondary: bytes, nitems: int, ngroups: int = 35) -> bytes:
    """Build the index region (primary + optional secondary + preamble)."""
    result = bytearray(primary)  # 140 bytes
    sec_entries = max(0, nitems - ngroups)
    if sec_entries > 0:
        # Keep first sec_entries from original secondary index
        result.extend(secondary[: sec_entries * 4])
        result.extend(b"\x00\x00\x64\x00\x00\x00\x64\x00\x00\x00")  # 10-byte preamble
    else:
        result.extend(b"\x00\x00\x00\x00\x00\x00")  # 6-byte preamble
    return bytes(result)


def generate(src_dir: str, dst_dir: str, max_records: int) -> dict[str, int]:
    """Truncate content files and write rebuilt mini fixtures."""
    os.makedirs(dst_dir, exist_ok=True)
    results: dict[str, int] = {}

    for fname in ADDON_FILES:
        src = os.path.join(src_dir, fname)
        if not os.path.exists(src):
            print(f"  SKIP {fname} — not found", file=sys.stderr)
            continue
        with open(src, "rb") as f:
            data = f.read()

        hdr = read_header(data)
        orig_items = hdr["nitems"]
        ngroups = hdr["ngroups"]
        dstart = data_start_offset(orig_items, ngroups)

        records = parse_records(data, dstart, orig_items)
        keep = min(max_records, len(records))
        if keep == len(records):
            keep = min(keep, max_records)

        # Extract primary index (140 bytes at offset 28)
        primary_idx = data[28 : 28 + PRIMARY_IDX_SIZE]

        # Extract original secondary index
        orig_sec_entries = max(0, orig_items - ngroups)
        sec_start = 28 + PRIMARY_IDX_SIZE
        secondary_idx = data[sec_start : sec_start + orig_sec_entries * 4]

        # Extract the data bytes for kept records
        first_rec = records[0]
        last_rec = records[keep - 1]
        data_bytes = data[first_rec[0] : last_rec[0] + last_rec[1]]

        # Rebuild the file
        new_hdr = dict(hdr)
        new_hdr["nitems"] = keep
        new_hdr["capacity"] = max(hdr["capacity"], keep)

        header_bytes = bytearray(build_header(new_hdr))
        index_bytes = build_indices(primary_idx, secondary_idx, keep, ngroups)

        # Compute new data_size
        # Header (28) + indices + data, then data_size = total - 8
        body = index_bytes + data_bytes
        total_size = 28 + len(body)
        new_data_size = total_size - 8
        struct.pack_into("<I", header_bytes, 0, new_data_size)
        struct.pack_into("<I", header_bytes, 20, new_data_size)

        truncated = bytes(header_bytes) + body

        dst = os.path.join(dst_dir, fname)
        with open(dst, "wb") as f:
            f.write(truncated)

        results[fname] = keep
        print(f"  {fname}: {keep}/{orig_items} records ({len(truncated)} bytes)")

    return results


def verify_reproducible(dst_dir: str, results: dict[str, int]) -> None:
    """Hash all generated fixtures for reproducibility check."""
    hashes = {}
    for fname in sorted(results):
        path = os.path.join(dst_dir, fname)
        with open(path, "rb") as f:
            hashes[fname] = hashlib.sha256(f.read()).hexdigest()
    return hashes


def main():
    parser = argparse.ArgumentParser(description="Generate W3 mini fixture tables")
    default_data = os.path.expanduser("~/.local/opt/pinyin-oracle/lib/libpinyin/data")
    parser.add_argument("--data-dir", default=default_data,
                        help="Oracle data directory")
    parser.add_argument("--max-records", type=int, default=100,
                        help="Maximum records per fixture (default: 100)")
    parser.add_argument("--output-dir", default=None,
                        help="Output directory (default: fixtures/w3/)")
    parser.add_argument("--hash-only", action="store_true",
                        help="Print SHA-256 of each fixture and exit")
    args = parser.parse_args()

    repo_root = Path(__file__).resolve().parent.parent
    dst_dir = args.output_dir or str(repo_root / "fixtures" / "w3")

    if args.hash_only:
        hashes = verify_reproducible(dst_dir, {})
        for name, h in sorted(hashes.items()):
            print(f"{h}  {name}")
        return

    if not os.path.isdir(args.data_dir):
        print(f"error: data directory not found: {args.data_dir}", file=sys.stderr)
        print("  Specify --data-dir or install the pinned oracle", file=sys.stderr)
        sys.exit(1)

    # Walk up from data dir to find oracle-pin.txt
    pin_file = None
    d = os.path.abspath(args.data_dir)
    for _ in range(4):
        candidate = os.path.join(d, "oracle-pin.txt")
        if os.path.exists(candidate):
            pin_file = candidate
            break
        d = os.path.dirname(d)
    pin_ref = "unknown"
    if pin_file:
        with open(pin_file) as f:
            for line in f:
                if "pin_ref=" in line:
                    pin_ref = line.strip()
                    break

    print(f"Pin ref: {pin_ref}", file=sys.stderr)
    print(f"Output:  {dst_dir}", file=sys.stderr)
    print(f"Generating addon content fixtures (max {args.max_records} records each):", file=sys.stderr)

    results = generate(args.data_dir, dst_dir, args.max_records)

    # Write pin stamp
    stamp_path = os.path.join(dst_dir, "pin-ref.txt")
    with open(stamp_path, "w") as f:
        f.write(f"{pin_ref}\n")

    hashes = verify_reproducible(dst_dir, results)
    hash_path = os.path.join(dst_dir, "fixtures.sha256")
    with open(hash_path, "w") as f:
        for name in sorted(results):
            f.write(f"{hashes[name]}  {name}\n")

    total = sum(results.values())
    print(f"\nDone: {total} records across {len(results)} files", file=sys.stderr)
    print(f"Pin stamp: {stamp_path}", file=sys.stderr)
    print(f"Checksums: {hash_path}", file=sys.stderr)


if __name__ == "__main__":
    main()
