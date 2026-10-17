# Startup baseline, 2026-10-17

Initialization cost (time, instructions, allocations and memory) of oxpinyin
against the pin, measured by `initbench` (`../runbooks/initbench.md`). The first
usable candidate is the outer startup boundary and is measured only to expose
initialization work deferred until first use. Nothing here is steady-state.

Dates are `date -u` of the run (2026-10-17). Evidence for what each phase does:
`../findings/startup-initialization-audit.md`.

## Scope and acceptance

Same backend (Berkeley DB), same system data (identity
`e18f23beb15c48ed0d86988364af70bdbce6840313fe1399317865cb811a56c2`, 23 files,
51 462 676 bytes), equivalent starting profiles, same API workload, and
**verified identical observable behaviour** before any timing. Equivalence,
performance and limitations are separate sections.

Artifacts. Every timed object, with its full SHA-256, size in bytes and
`NEEDED` list (`readelf -d`). The oxpinyin objects were built from this tree
with `cargo cinstall --release` (libpinyin: `--no-default-features --features
bdb,shipped`; libzhuyin: `--features bdb`); `ox-control` is the same bytes at
another path. The pin is libpinyin 2.11.92 at `074a2219`, Berkeley DB backend.

| subject | object | bytes | SHA-256 | NEEDED |
| --- | --- | ---: | --- | --- |
| pin | libpinyin.so.15.0.0 | 4802600 | `754499651c302efd91fe7f72dd6a791978400b268cb6206d2ebe644015721f30` | libglib-2.0, libdb-5.3, libstdc++, libm, libc, libgcc_s |
| pin | libzhuyin.so.15.0.0 | 4681128 | `cf4ccbce9f70f6c7e412b92a52a243ba64fdea1ff197cbc44019c6b85650d53b` | libglib-2.0, libdb-5.3, libstdc++, libm, libc, libgcc_s |
| ox, ox-control | libpinyin.so.15.0.0 | 5281016 | `8c57fb7d9b41b4e6421fca365cd19b72e3cf8a316137c7082a24d220c1ffd69b` | libglib-2.0, libdb-5.3, libgcc_s, libc, ld-linux-x86-64 |
| ox, ox-control | libzhuyin.so.15.0.0 | 5065096 | `712acf24bb6350e2984c730f6521d23c3725aa474dbcbf9f9e350610e8c3b771` | libglib-2.0, libdb-5.3, libgcc_s, libc, ld-linux-x86-64 |

Profiles (each sample gets a private copy). The fixtures have different shapes
and are **not** a controlled size-scaling series:

| profile | import calls | distinct phrases | frequency per row | libraries | user directory |
| --- | ---: | ---: | ---: | --- | ---: |
| `empty` | 0 | 0 | – | – | 0 B |
| `modest` | 80 | 2 (the same two in every library) | 5 | all four | 29 233 B |
| `phrases` | 10 000 | 10 000 | 20 000 | almost all `gb_char` | 496 209 B |
| `journals` | 10 000 | 10 000 | 20 000 | all four, 3000/3000/3000/1000 | 543 885 B |

20 000 is the **frequency** each row carries, not a record count. Each
non-empty fixture was accepted only after witness inputs answered differently
from `empty`. Results are therefore described per profile, never as a function
of size.

Host: Linux x86_64, 4 vCPU KVM guest, kernel 6.18, CPU pin 2, warm page cache
unless stated, Valgrind 3.22.0. 60 timed rounds (3 warm-up); 30 for memory,
cold and lazy. Benchmark binary SHA-256 `f17230c52c47d3bb…`, workload SHA-256
`cba003ff8827605b…`. 0 samples rejected for contamination.

## 1. Equivalence

20 of 20 (facade × profile × subject) cells, including the save/reopen
scenario, are **comparable**: return values, parsed length, candidate
counts/types/strings/order, sentences, stderr diagnostics, and files left in
the profile match (8 files compared by bytes and 3 by content in the
persistence scenario; 11 by bytes after a run). 12 hostile inputs per facade
ran one process each; none crashed.

One recorded difference, outside the timed workload: zhuyin `'` and `''`. The
pin's `zhuyin_guess_sentence` and `zhuyin_guess_candidates_after_cursor`
return false; oxpinyin returns true. It is the behaviour compatibility-policy
row 53 item 2 records as closed in code, so it contradicts the register; it was
reproduced at no option word, `0x20` and `0x18a`. Not fixed, not classified as
acceptable, not explained (see the audit). It does not affect the comparability
of the timed workload (the workload never parses these inputs) and is shown in
its own table.

## 2. Timing (milliseconds, warm cache, median [95% CI])

| facade | profile | metric | pin | ox | ox/pin |
| --- | --- | --- | ---: | ---: | ---: |
| pinyin | empty | library load | 1.80 | 0.90 | 0.50× |
| pinyin | empty | `pinyin_init` | 1.97 | 1.68 | 0.85× |
| pinyin | empty | time to first candidate | 3.91 | 2.79 | 0.71× |
| pinyin | modest | `pinyin_init` | 6.66 | 1.81 | 0.27× |
| pinyin | modest | time to first candidate | 8.46 | 2.87 | 0.34× |
| pinyin | phrases | `pinyin_init` | 3.13 | 21.29 | 6.79× |
| pinyin | phrases | time to first candidate | 4.88 | 22.62 | 4.63× |
| pinyin | journals | `pinyin_init` | 2.85 | 20.55 | 7.22× |
| pinyin | journals | time to first candidate | 4.54 | 21.66 | 4.77× |
| zhuyin | empty | `zhuyin_init` | 1.68 | 1.53 | 0.91× |
| zhuyin | empty | time to first candidate | 3.53 | 2.57 | 0.73× |
| zhuyin | modest | `zhuyin_init` | 6.46 | 1.73 | 0.27× |
| zhuyin | modest | time to first candidate | 8.28 | 2.87 | 0.35× |
| zhuyin | phrases | `zhuyin_init` | 2.58 | 20.44 | 7.92× |
| zhuyin | phrases | time to first candidate | 4.34 | 21.61 | 4.98× |
| zhuyin | journals | `zhuyin_init` | 2.69 | 22.29 | 8.28× |
| zhuyin | journals | time to first candidate | 4.44 | 23.58 | 5.31× |

The ox/pin ratios on `modest` look favourable to oxpinyin because the pin's own
`modest` init is slower than its own `phrases` init (anomaly and measured
cause in `../findings/startup-initialization-audit.md`). On `phrases` and
`journals` oxpinyin is slower at `*_init`. Do not read the table as a trend
over profile size.

Option setup and `*_alloc_instance` are 1–8 µs for both and negligible; the
first-result phase (parse, candidate list, first string) is 0.1 ms for the pin
and 0.2–0.34 ms for oxpinyin. Full tables with CIs and range/median spread are
in the report; the CIs are within ±10% of the medians shown. The ox-control /
ox ratio is 0.94–1.05 for init and time to first candidate in every cell, which
is the noise floor.

Time to first candidate is `end of first-result − start of library load`, from
one monotonic clock (`Instant`), and includes gaps between phases. It is not a
typing workload.

## 3. Instructions and allocations (Callgrind / DHAT, cumulative to each stop)

| facade | profile | subject | lib load Ir | init Ir | alloc calls at init |
| --- | --- | --- | ---: | ---: | ---: |
| pinyin | empty | pin | 4.12 M | 5.86 M | – |
| pinyin | empty | ox | 0.82 M | 1.45 M | – |
| pinyin | modest | pin | 4.12 M | 11.82 M | – |
| pinyin | modest | ox | 0.82 M | 1.85 M | – |
| pinyin | phrases | pin | 4.12 M | 10.34 M | 1 274 |
| pinyin | phrases | ox | 0.82 M | 140.90 M | 268 449 |

(`init Ir` is the delta of the `init` stop; zhuyin is within 1% of these.)
Instruction counts are of the process, include libdb and libc, and are not
comparable with C instruments' counts.

## 4. Memory at first candidate (MiB, memory-mode, median)

| facade | profile | pin RSS Δ | ox RSS Δ | pin anon | ox anon | pin peak | ox peak | pin faults | ox faults |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| pinyin | empty | 9.48 | 9.03 | 0.67 | 0.75 | 12.17 | 11.76 | 275 | 286 |
| pinyin | modest | 17.13 | 9.09 | 8.27 | 0.75 | 19.84 | 11.79 | 2223 | 289 |
| pinyin | phrases | 9.47 | 13.34 | 0.83 | 5.00 | 12.16 | 16.11 | 432 | 1785 |
| pinyin | journals | 9.48 | 13.36 | 0.88 | 5.03 | 12.18 | 16.26 | 440 | 1619 |

The pin's `modest` anonymous memory (8.27 MiB) is the same anomaly. zhuyin
differs by under 0.5 MiB in every cell. RSS includes shared file-backed
pages in full (PSS and private figures are in the report). Peak RSS is a
high-water mark.

## 5. Other conditions

Cold cache (30 rounds) and `RTLD_LAZY` (30 rounds) were run; the reports are
produced by the same command with `--cache cold` / `--bind lazy`. Cold means
`drop_caches` inside the guest, not cold hardware.

## Limitations and unmeasured cells

- **macOS: not run.** The code builds for it and the parsers are unit tested;
  no numbers exist. Windows: not verified.
- One host, one backend (BDB). kc and tkrzw cells: not measured.
- One CPU model, a KVM guest; absolute values do not transfer. Ratios are
  within-run only.
- The fixtures are not a controlled size-scaling series (different row counts,
  distinct phrases, frequencies and libraries). No claim is made about how init
  cost scales with profile size.
- The pin's `modest` init is slower and larger than its own `phrases` init.
  Per-function Callgrind on the pin shows the extra cost is in journal replay
  (`SubPhraseIndex::merge` → `add_phrase_item` → `MemoryChunk::set_content`);
  why the two journals take different paths is a hypothesis (audit §anomaly).
- Per-function attribution exists for the **pin** (one Callgrind run per profile,
  command in the audit) and **not** for oxpinyin: its figures here are
  whole-process per phase. The candidates in the audit are hypotheses.
- Cold-cache runs are guest-level.
- zhuyin `'`/`''` differs (see §1) and contradicts compatibility-policy row 53
  item 2, which records it as closed in code; unresolved, outside the timed
  workload, not fixed here.
- **Evidence.** Raw JSONL and profiler output are not committed and were not
  retained anywhere: the capture environment was an ephemeral container. The
  figures above are reproducible from the commands in
  `../runbooks/initbench.md`; the ones used here were
  `initbench run … --kind timing --rounds 60 --warmup 3` (§2),
  `--kind memory --rounds 30 --warmup 2` (§4), `--cache cold` and `--bind lazy`
  at 30 rounds (§5), and `initbench instrument --tool callgrind|dhat` once per
  facade, subject and profile (§3), each with `--cpu 2`, followed by
  `initbench report --input <file> --reference pin --ratio ox/pin --ratio
  ox-control/ox`.
