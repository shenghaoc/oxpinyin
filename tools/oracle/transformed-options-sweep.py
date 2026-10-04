#!/usr/bin/env python3
"""Reproducible Lane J replacement sweep; Python standard library only.

See transformed-options-sweep.md for the exact lists and comparison scope.
Each option word/library runs in a separate process with a fresh user profile.
No captures are written: full ordered observations are compared in memory.
"""

import argparse
import ctypes as c
import json
import pathlib
import subprocess
import sys
import tempfile

HERE = pathlib.Path(__file__).resolve().parent
CASES = json.loads((HERE / "transformed-options-inputs.json").read_text())
WORDS = [int(line, 16) for line in
         (HERE / "transformed-options-words.txt").read_text().splitlines()]


def observe(library, data, word, cases):
    """Observe parse, sentence and every ordered (type, UTF-8, nbest) row."""
    lib = c.CDLL(library)
    ptr, u32, size, boolean = c.c_void_p, c.c_uint32, c.c_size_t, c.c_bool

    def bind(name, result, *args):
        fn = getattr(lib, "pinyin_" + name)
        fn.restype, fn.argtypes = result, list(args)
        return fn

    init = bind("init", ptr, c.c_char_p, c.c_char_p)
    fini = bind("fini", None, ptr)
    alloc = bind("alloc_instance", ptr, ptr)
    free = bind("free_instance", None, ptr)
    options = bind("set_options", boolean, ptr, u32)
    schemes = {
        "hanyu": bind("set_full_pinyin_scheme", boolean, ptr, c.c_int),
        "luoma": bind("set_full_pinyin_scheme", boolean, ptr, c.c_int),
        "secondary": bind("set_full_pinyin_scheme", boolean, ptr, c.c_int),
        "double-ms": bind("set_double_pinyin_scheme", boolean, ptr, c.c_int),
        "chewing-standard": bind("set_zhuyin_scheme", boolean, ptr, c.c_int),
    }
    parsers = {}
    for scheme in schemes:
        parser = ("double" if scheme == "double-ms" else
                  "chewings" if scheme == "chewing-standard" else "full")
        suffix = parser if parser == "chewings" else parser + "_pinyins"
        parsers[scheme] = bind("parse_more_" + suffix, size, ptr, c.c_char_p)
    length = bind("get_parsed_input_length", size, ptr)
    sentence_guess = bind("guess_sentence", boolean, ptr)
    sentence_get = bind("get_sentence", boolean, ptr, c.c_uint8, c.POINTER(ptr))
    candidate_guess = bind("guess_candidates", boolean, ptr, size, u32)
    count = bind("get_n_candidate", boolean, ptr, c.POINTER(u32))
    candidate_get = bind("get_candidate", boolean, ptr, u32, c.POINTER(ptr))
    text_get = bind("get_candidate_string", boolean, ptr, ptr, c.POINTER(c.c_char_p))
    type_get = bind("get_candidate_type", boolean, ptr, ptr, c.POINTER(c.c_int))
    rank_get = bind("get_candidate_nbest_index", boolean, ptr, ptr, c.POINTER(c.c_uint8))
    glib = c.CDLL("libglib-2.0.so.0")
    glib.g_free.argtypes, glib.g_free.restype = [ptr], None
    records = []
    with tempfile.TemporaryDirectory(prefix="lane-j-sweep-") as user:
        context = init(data.encode(), user.encode())
        if not context:
            raise RuntimeError("pinyin_init failed")
        try:
            option_ok = bool(options(context, word))
            for scheme, number, text in cases:
                scheme_ok = bool(schemes[scheme](context, number))
                instance = alloc(context)
                if not instance:
                    raise RuntimeError("pinyin_alloc_instance failed")
                try:
                    consumed = int(parsers[scheme](instance, text.encode()))
                    parsed = int(length(instance))
                    guessed = bool(sentence_guess(instance))
                    sentence, got_sentence = None, None
                    if guessed:
                        out = ptr()
                        got_sentence = bool(sentence_get(instance, 0, c.byref(out)))
                        if out.value:
                            sentence = c.string_at(out).decode()
                            glib.g_free(out)
                    candidates_ok = bool(candidate_guess(instance, 0, 0x1E))
                    n = u32()
                    count_ok = bool(count(instance, c.byref(n)))
                    rows = []
                    for index in range(n.value):
                        candidate, candidate_type, candidate_text = ptr(), c.c_int(), c.c_char_p()
                        get_ok = bool(candidate_get(instance, index, c.byref(candidate)))
                        type_ok = bool(type_get(instance, candidate, c.byref(candidate_type)))
                        text_ok = bool(text_get(instance, candidate, c.byref(candidate_text)))
                        rank, rank_ok = c.c_uint8(), None
                        if candidate_type.value == 1:
                            rank_ok = bool(rank_get(instance, candidate, c.byref(rank)))
                        rows.append([get_ok, type_ok, text_ok, candidate_type.value,
                                     candidate_text.value.decode() if candidate_text.value else None,
                                     rank_ok, rank.value if rank_ok is not None else None])
                    records.append(dict(options=option_ok, scheme=scheme_ok,
                                        consumed=consumed, parsed=parsed,
                                        sentence_ok=guessed, sentence_get=got_sentence,
                                        sentence=sentence, candidates_ok=candidates_ok,
                                        count_ok=count_ok, n=n.value, rows=rows))
                finally:
                    free(instance)
        finally:
            fini(context)
    return records


def worker(library, data, word, cases):
    command = [sys.executable, str(pathlib.Path(__file__).resolve()),
               "--worker", library, data, hex(word), json.dumps(cases)]
    try:
        result = subprocess.run(command, capture_output=True, text=True, timeout=90, check=True)
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as exc:
        stderr = exc.stderr
        if isinstance(stderr, bytes):
            stderr = stderr.decode(errors="replace")
        print(f"worker failed: library={library}, word={hex(word)}, "
              f"inputs={json.dumps(cases, ensure_ascii=False)}\n"
              f"{exc}\nstderr:\n{stderr or '(empty)'}", file=sys.stderr)
        raise SystemExit(1) from exc
    return json.loads(result.stdout)


def encoded(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--oracle", required=True)
    parser.add_argument("--subject", required=True)
    parser.add_argument("--data", required=True)
    parser.add_argument("--expect", choices=["parent", "fixed"], required=True)
    parser.add_argument("--list-differences", action="store_true")
    args = parser.parse_args()
    modes = sorted({case[0] for case in CASES})
    ordinary = {mode: set() for mode in modes}
    complete = {mode: set() for mode in modes}
    for number, word in enumerate(WORDS, 1):
        pin = worker(args.oracle, args.data, word, CASES)
        ox = worker(args.subject, args.data, word, CASES)
        for case, left, right in zip(CASES, pin, ox, strict=True):
            mode = case[0]
            if encoded(left) != encoded(right):
                complete[mode].add(word)
            left_rows = [row[:5] for row in left["rows"] if row[3] != 1]
            right_rows = [row[:5] for row in right["rows"] if row[3] != 1]
            if encoded(left_rows) != encoded(right_rows):
                ordinary[mode].add(word)
        if number % 50 == 0:
            print(f"compared {number}/{len(WORDS)} option words", file=sys.stderr, flush=True)
    expected = {"hanyu": 0, "luoma": 437, "secondary": 437,
                "double-ms": 29 if args.expect == "parent" else 0,
                "chewing-standard": 31 if args.expect == "parent" else 0}
    summary = {mode: {"ordinary": len(ordinary[mode]), "complete": len(complete[mode])}
               for mode in modes}
    if args.list_differences:
        for mode in modes:
            summary[mode]["differing_words"] = [f"0x{word:08x}" for word in sorted(ordinary[mode])]
    print(json.dumps({"words": len(WORDS), "inputs": len(CASES), "schemes": summary}))
    assert {mode: len(ordinary[mode]) for mode in modes} == expected, "sweep counts moved"
    expected_complete = {**expected, "hanyu": 437, "luoma": 439, "secondary": 439}
    assert {mode: len(complete[mode]) for mode in modes} == expected_complete, "protocol counts moved"

    targeted = [("double-ms", 2, "nihk", 0x8002),
                ("chewing-standard", 1, "su3cl3", 0x8002),
                ("double-ms", 2, "n", 0x800A),
                ("chewing-standard", 1, "su", 0x8002)]
    targeted += [("hanyu", 1, text, word) for text in ["sh", "lishbakua"]
                 for word in [0x2, 0xA, 0x20, 0x28]]
    for mode, scheme, text, word in targeted:
        case = [[mode, scheme, text]]
        left = worker(args.oracle, args.data, word, case)[0]
        right = worker(args.subject, args.data, word, case)[0]
        changed = [key for key in left if encoded(left[key]) != encoded(right[key])]
        print(json.dumps({"scheme": mode, "input": text, "word": hex(word),
                          "pin_n": left["n"], "subject_n": right["n"],
                          "pin_consumed": left["consumed"], "subject_consumed": right["consumed"],
                          "differing_fields": changed}))
        if mode == "hanyu" and left["consumed"] == 0:
            # #542, lane C: do not treat empty-key sentence returns as #586.
            assert changed == ["sentence_ok", "sentence_get"]
        elif args.expect == "fixed" or mode == "hanyu":
            assert not changed, "targeted returned surface differs byte for byte"
        else:
            assert changed, "parent no longer reproduces the target defect"


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--worker":
        print(json.dumps(observe(sys.argv[2], sys.argv[3], int(sys.argv[4], 0),
                                 json.loads(sys.argv[5])), ensure_ascii=False))
    else:
        main()
