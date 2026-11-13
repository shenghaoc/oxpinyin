# oxpinyin

oxpinyin is a portable Rust re-expression of
[libpinyin](https://github.com/libpinyin/libpinyin).
The default build is the drop-in target: it replaces `libpinyin.so.15` and `libzhuyin.so.15`.
Its target is bug-for-bug compatibility with a pinned libpinyin, defects included.

The [source policy](docs/source-policy.md#stages) defines the stages.

**Status:** Stage 2: bug-for-bug compatibility, in progress ([roadmap](ROADMAP.md)).

(中文文档在 [这里](README.中文.md), 快看过来!)

## Platforms

The portable crates support Linux, macOS and Windows on x86_64 and arm64.
CI tests Linux x86_64 on each PR that changes code, and macOS arm64 and Windows x64 nightly and on dispatch.

## Quickstart

To contribute, start at [CONTRIBUTING.md](CONTRIBUTING.md).
The [setup guide](docs/runbooks/setup.md) builds and tests oxpinyin natively on Linux, macOS and Windows.

## Layout

| Path | What |
|---|---|
| `crates/*` | Workspace packages; see the [crate map](docs/architecture.md#crates) |
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
