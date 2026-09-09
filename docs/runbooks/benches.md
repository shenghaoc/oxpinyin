# Benches — criterion, the perf baseline, the RSS diagnosis, and profiles

Four tiers, none run by CI (`verify-nightly` has no perf lane): the
criterion benches for a component, the C-ABI perf baseline against the
oracle, the RSS diagnosis that decomposes a resident set rather than
sizing it, and a sampled profile of the keystroke cycle. Numbers are only
comparable within one container on one host; every published snapshot
(`docs/perf/`, `docs/findings/perf-*.md`) names both.

## Criterion benches

| crate | bench | needs |
| --- | --- | --- |
| oxpinyin-capi | `stage2` | the export (`PINYIN_EXPORT_DIR`), the C-ABI surface |
| oxpinyin-store | `backend_matrix_{tkrzw,kyotocabinet,lmdb,redb}` | `--features <backend>` (each is `required-features`-gated) |
| oxpinyin-store | `lmdb_bulk_load`, `redb_is_empty` | the named backend |
| oxpinyin-user | `export_phrases`, `phrase_read` | the export |
| pinyin-oracle | `scan_perf`, `dbm_bench`, `alloc_profile` | the export, model20 (`dbm_bench` also the bench oracles) |

Run one bench by name; without `--bench <name>` cargo also runs the lib
under libtest, which rejects criterion flags:

```sh
cargo bench -p oxpinyin-store --no-default-features --features redb --bench backend_matrix_redb
cargo bench -p oxpinyin-capi --bench stage2 -- --profile-time 10
```

A backend-specific criterion bench — one that names a peer's optional
dependency, such as heed for `lmdb` — must carry
`required-features = ["<backend>"]` in its `[[bench]]` entry. CI runs
`cargo clippy --workspace --all-targets` on the default backend, and
without it the target fails to resolve the dependency instead of being
skipped. Run one bench with `--bench <name>`; without it cargo also runs
the lib under libtest, which rejects criterion flags such as
`--profile-time`. To run a bench in debug mode: `cargo test --bench
<name>` (AGENTS.md points here).

## The perf baseline (oracle vs installed oxpinyin)

`tools/bisection/run-perf-baseline.sh` measures speed, installed size
and RSS with one dlopen harness (`bisect --perf`), alternating oracle
and oxpinyin processes. It needs the pin-built oracle, `cargo-c` and
the model cache. The reproducible form is the perf-matrix container:

```sh
docker build --platform linux/arm64 -f tools/bisection/Dockerfile.perf-matrix -t oxpinyin-matrix .
docker run --rm -v /tmp/perf-out:/out -e PINYIN_ORACLE_PREFIX=/opt/libpinyin-tkrzw \
    -e OXPINYIN_PERF_WORK=/out oxpinyin-matrix bash tools/bisection/run-perf-baseline.sh
```

Knobs: `PERF_RUNS`, `PERF_CYCLES`, `PERF_RAM_RUNS`, `PERF_CPU` (pin a
CPU; do it). `tools/bisection/run-perf-matrix.sh` is the four-backend
form. For a parent-vs-HEAD comparison use one `CARGO_TARGET_DIR` per
tree: cargo's freshness check is fooled by `git archive` mtimes and will
reuse the wrong artifacts otherwise.

## RSS diagnosis (where the resident set actually sits)

`bisect --perf` in `PERF_MODE=rss-diag` answers a different question from
the RAM modes: not how large the resident set is, but what it is made of.
At init and again after the keystroke cycles it reads
`/proc/self/smaps_rollup` (anonymous vs file-backed, the clean/dirty
split) and `mallinfo2()` (heap high-water, retained free chunks, arena
count), then calls `malloc_trim(0)` and reads both again — the one call
that separates allocator retention from memory genuinely held.

`tools/bisection/run-rss-diag.sh` drives the two cells round-robin over
**one** data directory (the drop-in configuration: both engines open
libpinyin's own installed `data/`) and writes `rounds.jsonl` plus an
`identity.txt` naming every artifact by SHA-256:

```sh
tools/bisection/run-rss-diag.sh \
    --lp /opt/libpinyin-kc/lib/libpinyin.so.15.0.0 \
    --ox <shipped oxpinyin .so> \
    --data /opt/libpinyin-kc/lib/libpinyin/data \
    --out /tmp/rss-diag --rounds 12 --cycles 8
```

Two rules the harness enforces and a reader must not undo:

* **`RSS_DIAG_DIR` belongs in its own round.** Setting it makes `bisect`
  write the `/proc/self/maps`, `/proc/self/smaps` and `malloc_info`
  dumps, and writing them allocates — which moves the very RSS the
  `malloc_trim` delta measures. The runner takes the measurement rounds
  with it unset and one mapping round per cell with it set.
* **RSS is backend-sensitive.** Two cells on one backend give a valid
  ratio; the absolute figures describe that backend's configuration and
  do not stand in for another's.

`tools/bisection/rss-smaps.py` turns a dump into a per-mapping `Rss`
table, and `--diff A B` into the per-mapping delta that names which
mappings carry a gap. `docs/findings/rss-attribution-2026-09-09.md` is
the worked example.

For live and peak **live** bytes on the Rust side, build the artifact
with the non-default `alloc-count` feature; `bisect` resolves the
`oxpinyin_alloc_*` readers with `dlsym` and reports `-1` when they are
absent, so the JSON shape does not change between cells. The feature is
never in a shipped artifact — `nm -D` is the check.

### Attributing a heap gap to callers

DHAT names the allocation *site*; naming its *caller* across the C-ABI
boundary is where this goes wrong, and the failure looks like a result.

**Valgrind's unwind of the dlopened Rust cdylib is not trustworthy above
`kcdbget`.** It resolves the libpinyin cell correctly, and on the
oxpinyin cell it produces symbol-table-plausible garbage — a chain with
`Arc<T>::drop_slow` calling `dict::ucs4_walk_key`, and
`pinyin_iterator_add_phrase` directly beneath `main`. Each address really
does sit inside the function it names, so nothing looks broken. None of
`--num-callers=24`, resolving at `addr - 1`, `debug = 2` with `lto =
false`, or `-C force-frame-pointers=yes` changes it; all produce
byte-identical stacks. Kyoto Cabinet debug symbols
(`libkyotocabinet16v5-dbgsym`) do fix the KC half and nothing else. The
worked case is
`docs/findings/rss-attribution-2026-09-09.md`, "DHAT's cross-FFI stacks
were wrong in a way that looked plausible".

**Use callgrind caller→callee edges instead** — exact call counts, no
stack walk. Two rules that produced wrong numbers before they were
caught:

* **Key on the callee's `(object, file, name)` triple, not the demangled
  name.** `BasicDB::get(char const*, unsigned long, char*, unsigned
  long)` is a substring of that function's own
  `::VisitorImpl::visit_full`, and summing both gave 575 where the answer
  was 152.
* **An inlined callee has no call edge.** `BasicDB::check` and most
  `BasicDB::get` sites inline into their caller, under-reporting by ~3×.
  Where inlining is possible, count something that cannot be inlined — a
  virtually-dispatched visitor, a heap allocation — and corroborate
  against a second such marker.

A profiling build may stand in for the shipped artifact here: block
counts were identical across the shipped recipe, `debug = 2` with LTO
off, and forced frame pointers.

## Profiles

```sh
tools/profile/run-w8-cycle.sh
```

Stages oxpinyin-capi through `cargo cinstall --profile profiling` (the
`profiling` profile keeps line tables and thin LTO) and samples the
keystroke cycle with `samply`, falling back to `cargo flamegraph` or
`perf`. Artifacts land in `target/profile/`;
`tools/profile/extract-hot-stacks.py` summarises them.
`PROFILE_REUSE_STAGE=1` skips the restage.

## Writing it up

A snapshot records the container image, the host, the pin, the exact
commands, and the controls (`docs/perf/perf-stage2-harness-2026-08.md`
is the template; `docs/findings/perf-keycost-first-alloc-2026-09-07.md`
a recent full example with confidence intervals and an acceptance
gate). A change that trades time for space or the reverse must say so
and justify it (AGENTS.md, "Source policy").

### Evidence: what is committed and what is not

**A findings document commits no captures.** Not DHAT JSON, not `/proc`
`smaps` dumps, not callgrind out-files, not `perf` data. They are large,
machine-generated, and unreviewable in a diff — and in at least one case
the bulk of the file is a part the document itself records as unusable.

What the document carries instead:

1. **The command that produced each figure.** Not each class of figure —
   each figure. Where two numbers come from different commands, different
   inputs, or different runs, they get separate entries, because a shared
   heading is exactly how one of them ends up with no provenance at all.
   That is what makes a number regenerable once the capture is gone, and
   it is the substitute for a path on a machine.
2. **The capture bundle, attached to the pull request**, with its URL *in
   the document* and not only in the PR description — a document that says
   "evidence is attached to the PR" without a link has the same problem as
   one pointing at `/tmp`. Record the bundle's SHA-256 alongside the link.
   A release asset is an **additional** mirror if you want one, never a
   substitute for the PR attachment.
3. **An explicit note where a figure cannot be regenerated**, *at the
   point of use*, so a weaker claim reads as one. Do not point at a path
   that no longer exists.

**The one exception to (2), and it is narrow.** If the capture environment
was ephemeral and the bundle could not be retained anywhere — no
attachment, no mirror, nothing to link — the document says so plainly, in
place of the link, and (1) carries the whole reproduction burden. Keep the
SHA-256 even then, so anyone still holding a copy can verify it. This is
not a licence to skip the attachment when attaching is possible: it exists
because it happened, in the record that prompted this rule, and a rule its
own founding case violates is worse than no rule. When it applies, (1) is
not "a Provenance section" but a complete recipe — toolchain, packages,
builds, measurement commands, and the reader invocations that turn the
captures into the document's tables.

Small, human-readable, directly-cited artifacts are the exception that
proves the rule — a SHA manifest, a pin file — and
`docs/findings/oracle-pin-074a221-evidence/` is what that looks like.
The line is bulk machine output, not evidence as such.

Derived readers stay in `tools/bisection/` (`rss-smaps.py`,
`cg-calls.py`): they are tools, not evidence, and they are what makes the
pipeline reproducible for anyone holding the bundle.
