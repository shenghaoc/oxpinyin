#!/usr/bin/env python3
"""Compare Lane I's saved bigram records and unigram field bytes.

DBM container headers/allocation differ; keys and values must match byte
for byte. Dump tools read the same-backend user_bigram.db without routing
through the phrase export iterator. Unigram logs have no DBM container,
so extract their unigram fields without numeric conversion. Pronunciation
fields are a separate state surface; report complete-log differences too.
Missing inputs fail, including zero cases.
"""

import argparse
import ctypes
from pathlib import Path
import re
import subprocess
import struct
import sys


def unigram_fields(path):
    # Pin 074a2219: storage/phrase_index_logger.h:158-213;
    # storage/phrase_index.h:124-125. Mirrors
    # oxpinyin-data/user_files.rs::decode_log_records, after MemoryChunk's
    # {u32 length, u32 XOR checksum} frame.
    data = path.read_bytes()
    length, checksum = struct.unpack_from("<II", data)
    if length != len(data) - 8:
        raise ValueError(f"bad MemoryChunk length: {path}")
    payload = data[8:]
    padded = payload + bytes((-len(payload)) % 4)
    calculated = 0
    for (word,) in struct.iter_unpack("<I", padded):
        calculated ^= word
    if calculated != checksum:
        raise ValueError(f"bad MemoryChunk checksum: {path}")
    offset, fields = 0, []

    def take(size):
        nonlocal offset
        end = offset + size
        if end > len(payload):
            raise ValueError(f"truncated log: {path}")
        value = payload[offset:end]
        offset = end
        return value

    def u16():
        return int.from_bytes(take(2), "little")

    while offset < len(payload):
        kind = int.from_bytes(take(4), "little")
        token = take(4)
        # The pin logs library-local slots; the subject may log the full
        # token. Resolve both to the public token identity for the export
        # below (user_files.rs::token_of / PHRASE_MASK), retaining the
        # original files and reporting their complete-byte differences.
        if kind != 4:
            library = {"gb_char": 1, "gbk_char": 2, "opengram": 3, "merged": 4}[path.stem]
            token = (int.from_bytes(token, "little") & 0xFFFFFF | library << 24).to_bytes(4, "little")
        if kind == 4:
            size = u16()
            if token != bytes(4) or size != 4:
                raise ValueError("invalid total-frequency record")
            fields.append(token + take(size) + take(size))
        elif kind == 3:
            old_size, new_size = u16(), u16()
            old, new = take(old_size), take(new_size)
            if min(old_size, new_size) < 6:
                raise ValueError("short phrase item")
            fields.append(token + old[2:6] + new[2:6])
        elif kind in (1, 2):
            item = take(u16())
            if len(item) < 6:
                raise ValueError("short phrase item")
            fields.append(token + bytes([kind]) + item[2:6])
        else:
            raise ValueError(f"unknown log type {kind}")
    result = b"".join(sorted(fields))
    path.with_suffix(".unigrams").write_bytes(result)
    return result


def bigrams(cell, profile):
    path = profile / "user_bigram.db"
    if cell == "bdb":
        raw = subprocess.check_output(["db5.3_dump", str(path)])
        lines = raw.split(b"HEADER=END\n", 1)[1].split(b"DATA=END", 1)[0].splitlines()
        if len(lines) % 2:
            raise ValueError("odd Berkeley DB dump")
        rows = [(bytes.fromhex(k.decode()), bytes.fromhex(v.decode()))
                for k, v in zip(lines[::2], lines[1::2])]
    elif cell == "kc":
        # 074a2219 storage/ngram_kyotodb.cpp:53-103 uses a StashDB
        # snapshot, not a file HashDB. Load it before listing its bytes.
        dumped = profile / "bigram-dump.kct"
        dumped.unlink(missing_ok=True)
        subprocess.run(["kchashmgr", "load", str(dumped), str(path)], check=True,
                       stdout=subprocess.DEVNULL)
        raw = subprocess.check_output(["kchashmgr", "list", "-pv", "-px", str(dumped)])
        dumped.unlink()
        rows = [tuple(bytes.fromhex(part.decode()) for part in line.split(b"\t"))
                for line in raw.splitlines()]
    else:
        dump = profile / "bigram.tsv"
        dump.unlink(missing_ok=True)
        subprocess.run(["tkrzw_dbm_util", "export", "--dbm", "hash", "--tsv",
                        "--escape", str(path), str(dump)], check=True,
                       stdout=subprocess.DEVNULL)
        # Escaped TSV is a lossless representation, compared within one
        # backend/tool version; sorting removes only the container walk order.
        rows = [tuple(line.split(b"\t")) for line in dump.read_bytes().splitlines()]
    if any(len(row) != 2 for row in rows):
        raise ValueError("malformed bigram dump")
    result = sorted(rows)
    (profile / "bigram.records").write_bytes(
        b"".join(k.hex().encode() + b"\t" + v.hex().encode() + b"\n" for k, v in result))
    return result


def read_unigrams(so, data, profile, tokens):
    """Read the saved profile back through the exported pinyin API.

    Use a separate process per library to avoid ELF symbol interposition.
    Both facade saves use the same storage format, so the pinyin accessor
    verifies zhuyin's saved data as well.
    """
    lib = ctypes.CDLL(so)
    ptr = ctypes.c_void_p
    lib.pinyin_init.argtypes = [ctypes.c_char_p, ctypes.c_char_p]
    lib.pinyin_init.restype = ptr
    lib.pinyin_alloc_instance.argtypes = [ptr]
    lib.pinyin_alloc_instance.restype = ptr
    lib.pinyin_token_get_unigram_frequency.argtypes = [ptr, ctypes.c_uint32,
                                                       ctypes.POINTER(ctypes.c_uint32)]
    lib.pinyin_token_get_unigram_frequency.restype = ctypes.c_bool
    lib.pinyin_free_instance.argtypes = [ptr]
    lib.pinyin_fini.argtypes = [ptr]
    ctx = lib.pinyin_init(data.encode(), profile.encode())
    if not ctx:
        raise ValueError("saved-profile init failed")
    inst = lib.pinyin_alloc_instance(ctx)
    if not inst:
        lib.pinyin_fini(ctx)
        raise ValueError("saved-profile instance failed")
    result = bytearray()
    try:
        for token in sorted(tokens):
            count = ctypes.c_uint32()
            if not lib.pinyin_token_get_unigram_frequency(inst, token, ctypes.byref(count)):
                raise ValueError(f"unigram export failed: {token:#x}")
            result.extend(struct.pack("<II", token, count.value))
    finally:
        lib.pinyin_free_instance(inst)
        lib.pinyin_fini(ctx)
    sys.stdout.buffer.write(result)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("cell", choices=["bdb", "kc", "tkrzw"])
    parser.add_argument("pin", type=Path)
    parser.add_argument("ox", type=Path)
    parser.add_argument("--pin-so")
    parser.add_argument("--ox-so")
    parser.add_argument("--data")
    parser.add_argument("--facade", choices=["pinyin", "zhuyin"])
    args = parser.parse_args()
    pin = sorted(p.name for p in args.pin.iterdir() if p.is_dir())
    ox = sorted(p.name for p in args.ox.iterdir() if p.is_dir())
    if not pin or pin != ox:
        raise ValueError(f"case coverage differs or is empty: pin={pin}, ox={ox}")
    if args.facade == "pinyin":
        for folder in (args.pin, args.ox):
            ranks = {int(rank) for rank in re.findall(
                r"coverage fixture=\d+ ordinal=\d+ nbest-index=(\d+)",
                (folder / "driver.err").read_text())}
            if ranks != {0, 1, 2}:
                raise ValueError(f"n-best coverage must include 0,1,2: {folder}: {ranks}")
        print("coverage: both pinyin sides chose indices 0,1,2")
    elif args.facade == "zhuyin" and len(pin) != 3:
        raise ValueError("zhuyin's sole sentence row must be chosen on all three inputs")
    status = 0
    for name in pin:
        left, right = args.pin / name, args.ox / name
        a, b = bigrams(args.cell, left), bigrams(args.cell, right)
        logs = sorted(p.name for p in left.glob("*.dbin"))
        if not logs or logs != sorted(p.name for p in right.glob("*.dbin")):
            raise ValueError(f"missing unigram logs in {name}")
        changed = [f for f in logs if unigram_fields(left / f) != unigram_fields(right / f)]
        full = [f for f in logs if (left / f).read_bytes() != (right / f).read_bytes()]
        # User-library chunks also carry unigram state. These cases never
        # train an imported phrase's pronunciation independently: require
        # the whole chunk to agree rather than ignoring that state.
        chunks = ("user.bin", "addon.bin", "network.bin")
        changed += [f for f in chunks if (left / f).read_bytes() != (right / f).read_bytes()]
        if args.pin_so:
            tokens = {0x010059F5}  # 今天: #603's no-op also survives reopen.
            for folder in (left, right):
                for filename in logs:
                    fields = unigram_fields(folder / filename)
                    for token, _, _ in struct.iter_unpack("<III", fields):
                        if token:
                            tokens.add(token)
            exports = []
            for so, folder in ((args.pin_so, left), (args.ox_so, right)):
                exported = subprocess.check_output([
                    sys.executable, __file__, "--read-unigrams", so, args.data,
                    str(folder), *[str(t) for t in sorted(tokens)]])
                (folder / "unigrams.api").write_bytes(exported)
                exports.append(exported)
            if exports[0] != exports[1]:
                changed.append("exported unigram API bytes")
        same = a == b and not changed
        print(f"{name}: {'IDENTICAL' if same else 'DIVERGENT'} "
              f"bigram-records={len(a)}/{len(b)} unigram-diffs={changed} "
              f"complete-log-diffs={full}")
        if not same:
            status = 2
    return status


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--read-unigrams":
        read_unigrams(*sys.argv[2:5], {int(t) for t in sys.argv[5:]})
        raise SystemExit(0)
    raise SystemExit(main())
