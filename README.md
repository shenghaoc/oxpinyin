# oxpinyin

oxpinyin is a portable Rust re-expression of
[libpinyin](https://github.com/libpinyin/libpinyin).
The default build is the drop-in target: it replaces `libpinyin.so.15`, and `libzhuyin.so.15` as libpinyin builds it with `--enable-libzhuyin`.
Its target is bug-for-bug compatibility with a pinned libpinyin, defects included.

The [source policy](docs/source-policy.md#stages) defines the stages.

**Status:** Stage 2: bug-for-bug compatibility, in progress ([roadmap](ROADMAP.md)).

(中文文档在 [这里](README.中文.md), 快看过来!)

## Platforms

Repository development supports Linux, macOS and Windows on x86_64 and arm64.
CI tests Linux x86_64 on each PR that changes code, and macOS arm64 and Windows x64 nightly and on dispatch.

## Performance

oxpinyin, the library code of #2278 (measured at 7e66a2e, whose library code is this PR's), against Kali's libpinyin 2.11.92-1 package. Both are built with Debian's flags: `hardening=+all`, and Debian's cargo wrapper, which keeps `lto = "fat"` and `panic = "abort"`. All counts are hardware instructions (`perf_event_open`, user space).

| Measure | oxpinyin | libpinyin | oxpinyin ÷ libpinyin |
|---|---:|---:|---:|
| Typing¹, instructions | 56,290,318 | 46,849,998 | 1.20 |
| Candidate path, instructions per steady cycle | 100,539,521 | 127,358,169 | 0.79 |
| Candidate path, steady time² | | | 0.52 |
| Start-up (cold open), instructions | 16,519,000 | 18,508,294 | 0.89 |
| Memory after typing, anonymous | 2,632 KiB | 2,868 KiB | 0.92 |
| Memory after typing, Pss | 14,426.5 KiB | 14,361 KiB | 1.00 |
| Install size, stripped (`strip --strip-unneeded`) `libpinyin.so.15` | 1,262,072 B | 927,824 B | 1.36 |

¹ Typing is every prefix of `nihaozhongguo`: parse, sentence guess and candidates. The single full-sentence guess takes 1,660,784 instructions against 1,379,644 (1.20).

² The ratio of the medians. The steady time of one run varies by 24.5% (libpinyin) and 27.0% (oxpinyin) across 20 runs on this host, so the table gives no absolute time.

Callgrind counts are not given. Callgrind counts one cold invocation, and its count differs from the hardware count by more than the cold start explains, so it does not measure the same thing.

Compiler: with libpinyin built by GCC 14 instead of GCC 16 on the same host, typing is 1.03 (56,230,730 against 54,384,760, default flags). With GCC 14, `std::__lower_bound<SingleGramItem>` (+7.81 M) and its comparator `token_less_than` (+1.75 M) become out-of-line calls, and `SingleGram::search` falls by 3.12 M; the ten functions that differ most explain 93% of the typing gap.

Host: Intel Core i7-9750H, WSL 2, Linux 6.18.58.1, amd64; Kali GNU/Linux Rolling 2026.2; GCC 16.2.0 (Debian 16.2.0-4); tkrzw 1.0.34; libpinyin pin 074a2219 (2.11.92).

Method: [docs/runbooks/benches.md](docs/runbooks/benches.md).

## Quickstart

To contribute, start at [CONTRIBUTING.md](CONTRIBUTING.md).
The [setup guide](docs/runbooks/setup.md) builds and tests oxpinyin natively on Linux, macOS and Windows.

## Layout

| Path | What |
|---|---|
| `src/`, `capi/`, `zhuyin-capi/`, `utils/`, `repl/`, `tool-support/` | Workspace packages; see the [crate map](docs/architecture.md#crates) |
| `ROADMAP.md` | Current stage, pin and open work |
| [docs/constitution.md](docs/constitution.md) | Project rules and supported external interfaces |
| [docs/source-policy.md](docs/source-policy.md) | Source-reading method, compatibility decisions and stage rules |
| `docs/runbooks/` | How to: backends, the oracle and differentials, Callgrind and Massif recipes, goldens and pins, releases |
| `docs/findings/` | Decisions and divergence records, indexed with a status for each file |
| `docs/testing/` | Corpus pipeline, oracle environment, fixture formats |
| `docs/packaging.md` | cargo-c packaging and installed-tree layout |
| `tools/oracle/` | Pin build recipe (`docs/runbooks/oracle.md`) |

See the [crate map](docs/architecture.md#crates) for package ownership.

To run or test a utility, select its Cargo package, for example:
`cargo run --locked -p oxpinyin-utils --bin ngseg -- --help`.

The nonshipping `oxpinyin-repl` is an interactive shell for the library's
test modes, such as `pinyin` and `parser2`. It runs on Linux and macOS and needs GLib, as `oxpinyin-utils` does.
Run `just repl pinyin` (or `just repl parser2 -- --parser zhuyin`).
Each invocation copies `fixtures/w3` to a temporary directory; training and
other writes are discarded on exit. Select a backend with `--backend bdb`
or `--backend kyotocabinet`. The two text-table modes also stage the authored `fixtures/datagen-toned`
text tables.

## Upstream

- Behaviour oracle: [libpinyin](https://github.com/libpinyin/libpinyin).
  The pin is in [`tools/oracle/oracle-pin.txt`](tools/oracle/oracle-pin.txt) ([pin identity](docs/runbooks/oracle.md#pin-identity)).
  Its [wiki](https://github.com/libpinyin/libpinyin/wiki) is the upstream documentation (n-gram model format, internals, multiple dictionaries).
- Frontend ABI and settings surface: [ibus-libpinyin](https://github.com/libpinyin/ibus-libpinyin).
  Its [wiki](https://github.com/libpinyin/ibus-libpinyin/wiki) documents the consumer's features and installation.

## Licence

GPL-3.0-or-later.
