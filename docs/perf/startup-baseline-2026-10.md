# Startup baseline, 2026-10-17

Initialization and time to first candidate, oxpinyin against the pin, measured
by `initbench` (`../runbooks/initbench.md`). Nothing here is steady-state.

Dates are `date -u` of the run (2026-10-17). Evidence for what each phase does:
`../findings/startup-initialization-audit.md`.

## Scope and acceptance

Same backend (Berkeley DB), same system data (identity
`e18f23beb15c48ed0d86988364af70bdbce6840313fe1399317865cb811a56c2`, 23 files,
51 462 676 bytes), equivalent starting profiles, same API workload, and
**verified identical observable behaviour** before any timing. Equivalence,
performance and limitations are separate sections.

Artifacts (SHA-256):

| subject | libpinyin.so.15.0.0 | libzhuyin.so.15.0.0 |
| --- | --- | --- |
| pin (libpinyin 2.11.92 @ `074a2219`, BDB) | `754499651c302efd…` | `cf4ccbce9f70f6c7…` |
| ox (this tree, `cargo cinstall --release`, `--no-default-features --features bdb,shipped`; zhuyin `--features bdb`) | `8c57fb7d9b41b4e6…` | `712acf24bb6350e2…` |
| ox-control (same bytes as ox at another path) | same | same |

Profiles (all private copies, one per sample): `empty` (0 B), `modest` (80
imports, 29 233 B), `phrases` (20 000 imports, 496 209 B), `journals` (journal
files present, 543 885 B). Each non-empty fixture was accepted only after
witness inputs answered differently from `empty`.

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
return false; oxpinyin returns true. See the audit. It does not affect the
comparability of the timed workload and is shown in its own table.

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

zhuyin differs by under 0.5 MiB in every cell. RSS includes shared file-backed
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
- The pin's `modest` init is slower and larger than its own `phrases` init;
  the cause is not established (see the audit). It makes the `modest` ratios
  look favourable to oxpinyin; they are measured but are not a general trend.
- Instruction attribution by function inside oxpinyin's init was not captured;
  the ranking in the audit is by phase, not by function.
- Cold-cache runs are guest-level.
- zhuyin `'`/`''` differs (see §1); it is outside the timed workload.
- Raw JSONL and profiler output are not committed.
