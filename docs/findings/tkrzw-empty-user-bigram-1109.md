# Skipping the empty tkrzw user-bigram walk (#1109)

Date: 2026-10-23 UTC (`date -u` at run time). Change:
`TkrzwUserBigramDb::load_db` leaves `user_bigram.db` unwalked only when the
file holds no record (`holds_no_record` in
`crates/oxpinyin-store/src/tkrzw/mod.rs`). That needs all three of:

- the file opens healthy;
- its header counts 0 records;
- `fstat` shows the file ends at or before the record section that its
  header places.

Every other file is walked as before. This is option 1 of
[#1099](tkrzw-init-1099.md), approved by the maintainer. The size
condition is the maintainer's ruling on review. The first version
trusted the header's record count alone.

Base is the #1108 head (676d1ca5, one commit with the tree of e6a8171).
The code commit is c878ba5b. Figures for that code come from one host,
17:55Z–18:47Z. Figures marked *earlier head* come from the first
version (caac788, 13:11Z–14:18Z) and were not repeated. The scripts are at the end of this note and in
[tkrzw-init-1099.md](tkrzw-init-1099.md#scripts). Raw outputs stayed in
`/dev/shm` in a disposable cloud container and were deleted.

## Summary

- **Init Ir falls to about 3% of base and of the pin:** PR/base and
  PR/pin are 0.0300–0.0313 in all four cells, about 329.26M fewer
  instructions. These are the earlier head's ratios, so the size check
  leaves the gain unchanged. The control equals PR exactly. The 329,255,106-Ir bucket
  walk appears in the base and pin profiles and in neither the PR nor the
  control profile.
- **Init time (earlier head).** Warm init medians are 2.9–3.1 ms for PR, against
  53.7–56.3 ms for base and 51.5–56.4 ms for the pin. One figure passes the
  §7.5 control rule: pinyin `phrases` init CPU, PR/base **0.0576**
  (95% CI 0.0564–0.0606) and PR/pin **0.0589** (0.0559–0.0614). The other
  seven init figures are INCONCLUSIVE.
- **The pin's state is unchanged.** The source shows that the skipped
  walk would have copied nothing:
  - the file holds no record bytes at all, whatever its header counts;
  - a header that counts 0 records over a record section that holds
    records is walked, and the new unit test keeps all three of its
    records;
  - the `contract-diff` gate passes on tkrzw, BDB and KC (2,889 cases
    each), and so does `same-data-dir-diff`.

  On the earlier head:
  - a mutation that skips every file failed 128 contract cases;
  - saved user files were byte-identical between base and PR in 176 of
    176 comparisons;
  - `user_bigram.db` agreed with the pin in 16 of 16 (tkrzw's
    wall-clock timestamp masked in all of them).
- **Pre-existing, outside this change:** oxpinyin's saved
  `user_pinyin_index.bin` and `user_phrase_index.bin` differ from the pin's
  in two bytes. They hold the HashDBM `cyclic_magic` metadata-write counter
  (pin 7, ox 5). Base and PR are identical there.
- **Size.** Stripped libpinyin grows by one 4 KiB page, from 2,487,008 B
  to 2,491,104 B. Stripped libzhuyin is unchanged at 2,245,344 B.

## Why the container is the pin's

At 074a2219, `Bigram::load_db` creates a `TinyDBM`, opens the file
read-only and copies it with `copy_tkrzwdb` (`ngram_tkrzwdb.cpp:48-63`,
`tkrzwdb_utils.h:48-74`). The copy changes the container only through
`Set`, once per record that the iterator yields. If the iterator yields
none, the container is the freshly constructed `TinyDBM`.

Read at tkrzw 9ee8416 (1.0.34):

- **The count is exact.** `num_records_` changes only under the record
  lock: +1 for a new key, −1 for removing a live one
  (`HashDBMImpl::ProcessImpl`, `tkrzw_dbm_hash.cc:1572-1583`, `:1632`).
- **The closed flag goes to disk with that count.** It is written
  only under the database's exclusive lock: at creation (`:1180-1186`), in
  `SynchronizeImpl`, which sets it and clears it again before releasing the
  lock (`:738-766`), and in `CloseImpl` (`:1236-1243`).
- **A read-only open checks it.** `OpenImpl` reads the file as healthy
  only if that flag is set and the file size agrees with the metadata
  (`:1202-1216`).
- **The C calls report exactly these values.** `tkrzw_dbm_is_healthy` and
  `tkrzw_dbm_count` return `HashDBMImpl::IsHealthy` (`open_ && healthy_`,
  `:845-848`) and `HashDBMImpl::Count` (`num_records_`, `:619-626`), through
  `PolyDBM` (`tkrzw_langc.cc:1230-1243`, `:1464-1473`).

So a healthy file with a count of 0 was closed with no live record, and
no record write followed. That argument trusts the header's count. A file
whose header bytes were edited to count 0 records, while its record
section still holds records, would open healthy and still hold records
for the pin's walk to copy. The skip therefore also checks the file
itself. Records live only in the record section, which starts at
`record_base_`. `HashDBMImpl::SetRecordBase` (`tkrzw_dbm_hash.cc:1351-1356`)
computes that offset from three header fields, `num_buckets`,
`offset_width` and `align_pow`:
`AlignNumber(128 + num_buckets × offset_width + 1008 + 16, max(4096, 1 << align_pow))`.
`HashDBM::ReadMetadata` reads the fields as big-endian numbers at
offsets 16, 13 and 14 (`:39-42`, `:2676-2679`).

`holds_no_record` reads the same three fields from the file's first 128
bytes and computes that offset. It skips the walk only when `fstat` of
the open file shows the file ends at or before it. Such a file has no
record bytes, so no iterator over it can find a record, whatever its
header counts. The pin's copy then calls `Set` zero times. `load_db`
returns the same freshly constructed container without walking. The
container has no other state, so every later read, walk and save is the
same as before.

An empty 1,048,583-bucket file of width 4 and `align_pow` 3 ends exactly
at its record section: 4,198,400 B, as #1108 measured. Every provisioned
tkrzw profile's `user_bigram.db` is that file, so it is still skipped.

Every other file is still walked. That includes an unhealthy file, a file
whose header counts records, a file with bytes past its record section,
and a file whose count or header cannot be read. The unit tests:

- `a_file_saved_with_no_rows_holds_no_record_and_loads_empty`: an empty
  saved file is 4,198,400 B, its computed record section starts at
  4,198,400, it is skipped, and it loads empty.
- `a_file_saved_with_rows_is_walked`.
- `an_unhealthy_file_is_walked_even_when_its_metadata_counts_no_rows`: a
  hash file copied while its writer still has it open. Its metadata reads
  0 records and not closed, while its buckets reach three records.
  `load_db` copies all three. On the earlier head, a named mutation that
  dropped the health check failed this test alone.
- `a_healthy_zero_count_header_over_live_records_is_walked`: a cleanly
  saved three-record file whose header record count alone is rewritten to
  0. It opens healthy and counts 0, but it is larger than its record
  base. It is walked and keeps all three records. The named mutation
  that ignores the size condition (`ends_before_records(path) || true`)
  fails this test alone.

## Verification

### Unit tests and lints

`source ~/.oxpinyin-oracle.env`. The tkrzw builds use setup's libtkrzw
through `PKG_CONFIG_PATH`.

```sh
cargo fmt --all --check
python -m tools.dev slot run -- cargo test --locked -p oxpinyin-store --no-default-features --features tkrzw
python -m tools.dev slot run -- cargo clippy --locked -p oxpinyin-store --no-default-features --features tkrzw --all-targets -- -D warnings
python -m tools.dev slot run -- cargo clippy --locked -p oxpinyin-store --all-targets -- -D warnings
```

- `cargo test`: 50 library tests passed, including the four
  `tkrzw::tests`, and the trait-law suite's 6.
- Both clippy runs are clean.

### Contract and same-data gates

```sh
python tools/bisection/run-all.py --runners contract-diff,same-data-dir-diff --cells bdb,kc,tkrzw \
  --oracle bdb="$OXPINYIN_ORACLE_PREFIX_BDB" --oracle kc="$OXPINYIN_ORACLE_PREFIX_KC" \
  --oracle tkrzw="$OXPINYIN_ORACLE_PREFIX_TKRZW" --target-root "$W/cells-1110" \
  --runner-timeout contract-diff=14400 --runner-timeout same-data-dir-diff=7200
```

Run on c878ba5b, 17:55Z–18:47Z:

| runner | cell | status | cases | seconds |
| --- | --- | --- | ---: | ---: |
| `contract-diff` | bdb | PASS | 2,889 (5,776 workers) | 1,463.9 |
| `contract-diff` | kc | PASS | 2,889 (5,752 workers) | 1,495.3 |
| `contract-diff` | tkrzw | PASS | 2,889 (5,752 workers) | 1,583.8 |
| `same-data-dir-diff` | bdb | PASS | — | 17.9 |
| `same-data-dir-diff` | kc | PASS | — | 13.2 |
| `same-data-dir-diff` | tkrzw | PASS | — | 14.6 |

`run-all.py` built each debug cell from the PR tree. The tkrzw cell's
libraries resolve one `libtkrzw.so.1`, setup's 1.0.34. The registry's
900 s timeouts were raised for this 4-CPU host.

**Sensitivity of the gate (earlier head).** In a detached worktree with the named
mutation "skip-always" (`load_db` skips every file it opens), the same
`contract-diff` command fails. Of 2,889 cases, 101 DIFFER and 27 are
INVALID; 2,744 MATCH, 13 SKIP and 4 are HOST-DEPENDENT. The failing cases
are the user-bigram families: `d18-*` 34, `removal-*` 23, `abort-*` 20,
`replay-*` 20, `user-*` 18, `mask-*` 6, and 7 others. So the gate reads
the results of loading `user_bigram.db` with rows. The runner log was
parsed in RAM and deleted.

### Saved files, byte for byte (earlier head)

The tkrzw `contract-diff` compares a saved `user_bigram.db` only by
presence (`len(raw) > 0`, `contract-diff.py`'s user-file reader), so the
bytes were compared separately. Command: [`verify-saves.py`](#verify-savespy)
with sides pin, base and PR (the release libraries below). Each side
runs the existing teardown worker, `time 1 1 1 dirty <rows>`, twice in
sequence on its own copy of a profile. The rows file is teardown's own
dirty row. Each run initializes, takes the first candidate, imports the
row, saves and finalizes. The second run loads what the first saved. The
starting profiles were:

- `phrases` and `journals`: a healthy `user_bigram.db` with 0 records,
  the skip path;
- the 1,000-record synthetic file from #1108's scaling, the walk path;
- `phrases` without `user_bigram.db`: the second run takes the skip path.

Every user file was masked only where a writer stamps the wall clock:
bytes 48–55 of a tkrzw file (`META_OFFSET_TIMESTAMP`) and bytes 4–7 of
`user.bin`, `addon.bin` and `network.bin`, as `contract-diff` masks them.

| comparison | result |
| --- | --- |
| base vs PR, every file (11 files, 2 facades, 4 profiles, 2 saves) | 176 of 176 identical |
| pin vs base vs PR, `user_bigram.db` | 16 of 16 identical (4,198,400 B on the skip path; 4,222,400 B with the 1,000 rows) |
| pin vs base vs PR, every file other than the two below | identical |
| pin vs ox, `user_pinyin_index.bin` and `user_phrase_index.bin` | differ in 2 bytes, offsets 9 and 127 (`cyclic_magic`: pin 7, ox 5), in every profile, with this change or without it |

The `cyclic_magic` difference comes from the save path of the two
TreeDBM user indexes, which this change does not touch. tkrzw counts
metadata writes in that byte, and the pin's save writes the metadata twice
more than oxpinyin's. The rows match, which is what `contract-diff`
compares. It is reported for its own issue.

### Path check (AGENTS.md verification rule 2)

- `git diff --name-only origin/main...HEAD` lists
  `crates/oxpinyin-store/build.rs` and
  `crates/oxpinyin-store/src/tkrzw/mod.rs`, plus #1108's findings document.
- The non-Markdown `feature = "…"` grep lists
  `crates/oxpinyin-store/build.rs`.
- The Cargo.toml backend-line check prints nothing.

The check therefore names all three cells, and the gates ran on all
three (above). Every changed line sits in `mod tkrzw`, which compiles only
with `feature = "tkrzw"` (`build.rs`, `src/lib.rs`).

## Measurements

Libraries for the measurements:

- **base:** built from 676d1ca5, the #1108 head; identical code to main
  at its base.
- **PR:** built from c878ba5b.
- **control:** the PR library run again as its own side.
- **pin:** the tkrzw oracle.

Both oxpinyin builds used the same recipe: compatibility image
`oxpinyin-compat:ddd2c1a8e182d4d3`, GNU ld 2.47, cargo-c 0.10.24, and
setup's libtkrzw 1.0.34 through `PKG_CONFIG_PATH`. Each was built from a
`git archive` with its own target directory. Every library resolves one
`libtkrzw.so.1`, setup's.

| library | base sha256 | PR sha256 | stripped base / PR |
| --- | --- | --- | --- |
| libpinyin.so.15.0.0 | `776e48a2db31…` | `c0d23e4d6897…` | 2,487,008 / 2,491,104 B |
| libzhuyin.so.15.0.0 | `7a35c0a0ef43…` | `fdc93bb956a4…` | 2,245,344 / 2,245,344 B |

The base hashes equal the earlier head's base. Stripped libpinyin is one
4,096-byte page larger than base; libzhuyin is the same size.

### Init instructions

Init is the `<facade>_init` toggle plus the load constructors, as in
#1096 and #1108. Command:
[`attrib.py`](tkrzw-init-1099.md#attribpy), with base, PR, control and
pin each copied to an equal-length scratch path, on the tkrzw
`provision` profiles. It ran three rounds over the four cells in one
job, 17:55:37Z–18:06:28Z, with the same in-tree worker for every side.

| cell | base | PR | control | pin | PR/base | PR/pin | base − PR |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| pinyin `phrases` | 339,893,626 | 10,638,884 | 10,638,884 | 340,074,661 | 0.0313 | 0.0313 | 329,254,742 |
| pinyin `journals` | 339,890,792 | 10,635,417 | 10,635,417 | 340,143,282 | 0.0313 | 0.0313 | 329,255,375 |
| zhuyin `phrases` | 339,452,955 | 10,197,248 | 10,197,248 | 339,700,559 | 0.0300 | 0.0300 | 329,255,707 |
| zhuyin `journals` | 339,451,418 | 10,195,747 | 10,195,747 | 339,771,982 | 0.0300 | 0.0300 | 329,255,671 |

- Each side gave one value in all three rounds. Control equals PR exactly,
  so the control is VALID (§7.5 item 3).
- `HashDBMImpl::ReadNextBucketRecords` costs 329,255,106 Ir inclusive in
  every base and pin profile. It appears in neither the PR profile nor the
  control profile.
- PR/base and PR/pin equal the earlier head's to four decimals, so the
  gain is unchanged. The earlier head's absolute figures came from another
  run and worker build, and are not compared here.

### Warm init time (earlier head)

Command: [`warmtime4.py`](#warmtime4py) with cases `phrases,journals`,
run 14:16:29Z–14:17:47Z; the 1-minute load average was 0.64 before and
0.97 after. The script uses teardown's own `Campaign` and its §7.5
functions:

- 60 timed rounds after 3 warm-ups, with base, PR, control and pin
  rotating;
- one worker process per sample, pinned to CPU 2, with a fresh profile
  copy;
- all four sides must produce the same workload observation; all four
  cells were COMPARABLE;
- the control rule is control/PR (4,000 resamples, seed `0x074A2219`), and
  PR/base and PR/pin are given only where it passes.

| cell | clock | base µs | PR µs | control µs | pin µs | control/PR (95% CI) | verdict | PR/base (95% CI) | PR/pin (95% CI) |
| --- | --- | ---: | ---: | ---: | ---: | --- | --- | --- | --- |
| pinyin `phrases` | wall | 53,709 | 3,041 | 3,023 | 52,661 | 0.992 (0.960–1.057) | INCONCLUSIVE | — | — |
| pinyin `phrases` | CPU | 52,782 | 2,997 | 3,003 | 51,515 | 1.000 (0.960–1.029) | STABLE | **0.0576 (0.0564–0.0606)** | **0.0589 (0.0559–0.0614)** |
| pinyin `journals` | wall | 53,948 | 3,107 | 3,024 | 54,436 | 0.973 (0.915–1.020) | INCONCLUSIVE | — | — |
| pinyin `journals` | CPU | 53,672 | 3,088 | 3,026 | 53,617 | 0.973 (0.913–1.018) | INCONCLUSIVE | — | — |
| zhuyin `phrases` | wall | 56,315 | 2,944 | 2,965 | 56,405 | 0.999 (0.922–1.058) | INCONCLUSIVE | — | — |
| zhuyin `phrases` | CPU | 53,958 | 2,904 | 2,909 | 53,982 | 1.008 (0.941–1.043) | INCONCLUSIVE | — | — |
| zhuyin `journals` | wall | 54,371 | 2,926 | 2,635 | 53,755 | 0.934 (0.892–0.972) | INCONCLUSIVE | — | — |
| zhuyin `journals` | CPU | 53,624 | 2,879 | 2,606 | 53,057 | 0.935 (0.922–0.971) | INCONCLUSIVE | — | — |

The medians of INCONCLUSIVE rows are what the tool printed. They carry no
ratio. The same run's full lifecycle (load to `dlclose`) in pinyin
`phrases` CPU is STABLE at PR/base 0.1322 (0.1249–0.1346).

### Time and space

Time falls on both axes measured. The change adds no heap allocation and
no stored state. The size check opens the file, reads its 128-byte header
into a stack buffer and calls `fstat` once. It removes the read of the
whole 4 MiB bucket array, so init touches fewer file pages; Pss and Rss
were not measured.

## What this does not show

- Cold cache and memory.
- Profiles with learned bigrams. Their `user_bigram.db` has records, so
  they are walked as before and pay the full walk. Option 2 of #1099
  (deferred) is the change that would cover them.
- Another CPU or arm64.

## Scripts

These ran from the session's scratch directory with the repository at
`/home/user/oxpinyin`, through
`python -m tools.dev slot run -- uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal <script> …`.
They are shown condensed: docstrings are removed and some lines are
joined. The calls and the arithmetic are the ones that ran.

### warmtime4.py

Run as `warmtime4.py $W/timing4 "$OXPINYIN_ORACLE_DATA_TKRZW" $W/teardown/profiles 2 $M/base $M/subj $M/ctrl $M/pinl phrases,journals`.

```python
import runpy, sys, types
from pathlib import Path

ROOT = Path('/home/user/oxpinyin')
sys.path.insert(0, str(ROOT / 'tools/bisection'))
import teardown

CD = runpy.run_path(str(ROOT / 'tools/bisection/contract-diff.py'), run_name='contract_diff')
CHECK = runpy.run_path(str(ROOT / 'tools/tkrzw/runtime.py'))['check']
SIDES = ('base', 'ox', 'control', 'pin')


def main():
    work, data, profiles, cpu, base, pr, control, pin, cases = sys.argv[1:10]
    work, data, profiles = Path(work), Path(data), Path(profiles)
    driver = types.SimpleNamespace(**{name: CD[name] for name in
                                      ('check_cancelled', 'terminate_group', 'index_file_rows', 'zero_item_padding')})
    worker, identity = teardown.build_worker(work)
    teardown.print_record('worker', worker=identity)
    campaign = teardown.Campaign(worker, data, work, int(cpu), driver)
    dirs = dict(base=Path(base), ox=Path(pr), control=Path(control), pin=Path(pin))
    for facade in ('pinyin', 'zhuyin'):
        subjects = {side: dirs[side] / ('lib' + facade + '.so') for side in SIDES}
        for side, library in subjects.items():
            CHECK('auto', 'warmtime4.py', side, library)
        for case in cases.split(','):
            profile = profiles / case / 'user'
            references = {side: teardown.workload_observation(campaign.run(facade, library, profile, mode='time'))
                          for side, library in subjects.items()}
            comparable = all(value == references['pin'] for value in references.values())
            teardown.print_record('equivalence', facade=facade, case=case,
                                  verdict='COMPARABLE' if comparable else 'NOT COMPARABLE',
                                  digests={side: teardown.identity(value) for side, value in references.items()})
            if not comparable:
                continue
            samples = {side: [] for side in SIDES}
            for index in range(teardown.TIMING_ROUNDS + teardown.TIMING_WARMUPS):
                for side in list(SIDES[index % 4:] + SIDES[:index % 4]):
                    row = campaign.run(facade, subjects[side], profile, mode='time')
                    if teardown.workload_observation(row) != references[side]:
                        raise ValueError(f'timed workload observation changed: {facade} {case} {side}')
                    if index >= teardown.TIMING_WARMUPS:
                        samples[side].append(row)
            teardown.report_times(facade, case, {side: samples[side] for side in ('ox', 'control', 'pin')}, False)
            for metric in samples['ox'][0]['samples'][0]['times']:
                for clock in ('wall_ns', 'cpu_ns'):
                    values = {side: [row['samples'][0]['times'][metric][clock] for row in rows]
                              for side, rows in samples.items()}
                    control_check = teardown.stable_control(values['ox'], values['control'])
                    teardown.print_record('time-base', facade=facade, case=case, metric=metric, clock=clock,
                                          base=teardown.summary(values['base']),
                                          verdict='STABLE' if control_check['stable'] else 'INCONCLUSIVE',
                                          pr_base_ratio=teardown.paired_ratio(values['ox'], values['base'])
                                          if control_check['stable'] else None)


if __name__ == '__main__':
    main()
```

### verify-saves.py

Run as `verify-saves.py $W/bisect "$OXPINYIN_ORACLE_DATA_TKRZW" $W/saves $W/rows pin=$M/pinl base=$M/base pr=$M/subj -- <profile user dirs>`.
`$W/rows/dirty-{pinyin,zhuyin}.tsv` hold teardown's dirty row
(`1\t测试\tce4'shi4\t20000` and `1\t测试\tㄘㄜˋ ㄕˋ\t20000`).

```python
import hashlib, os, runpy, shutil, subprocess, sys
from pathlib import Path

ROOT = Path('/home/user/oxpinyin')
GEN = runpy.run_path(str(ROOT / 'tools/generate-function-order.py'), run_name='gen')
CHECK = runpy.run_path(str(ROOT / 'tools/tkrzw/runtime.py'))['check']


def masked(path):
    raw = bytearray(path.read_bytes())
    if raw.startswith(b'TkrzwHDB\n'):
        raw[48:56] = bytes(8)  # HashDBM metadata timestamp
    elif path.name in ('user.bin', 'addon.bin', 'network.bin'):
        raw[4:8] = bytes(4)  # contract-diff's user_files mask
    return hashlib.sha256(raw).hexdigest(), len(raw)


def main():
    worker, data, work, rows_dir = map(Path, sys.argv[1:5])
    rest = sys.argv[5:]
    split = rest.index('--')
    sides = dict(item.split('=', 1) for item in rest[:split])
    profiles = [Path(item) for item in rest[split + 1:]]
    disagreements = 0
    for facade in ('pinyin', 'zhuyin'):
        rows = rows_dir / ('dirty-' + facade + '.tsv')
        for profile in profiles:
            states = {}
            for index, (side, directory) in enumerate(sides.items()):
                library = Path(directory) / ('lib' + facade + '.so')
                env = dict(os.environ, LC_ALL='C', OXPINYIN_TEARDOWN_EXPORTS=GEN['teardown_exports'](library, facade))
                CHECK('auto', 'verify-saves.py', side, library, env=env)
                scratch = work / ('saves-%d' % index)
                if scratch.exists():
                    shutil.rmtree(scratch)
                shutil.copytree(profile, scratch / 'user')
                runs = []
                for _ in range(2):
                    subprocess.run([str(worker), '--teardown', facade, str(library), str(data), str(scratch / 'user'),
                                    'time', '1', '1', '1', 'dirty', str(rows)],
                                   env=env, check=True, capture_output=True, timeout=600)
                    runs.append({p.name: masked(p) for p in sorted((scratch / 'user').iterdir()) if p.is_file()})
                states[side] = runs
                shutil.rmtree(scratch)
            for run in range(2):
                for name in sorted(set().union(*(states[side][run] for side in sides))):
                    values = {side: states[side][run].get(name) for side in sides}
                    agree = len(set(values.values())) == 1
                    disagreements += not agree
                    print(facade, profile.parent.name, 'save', run + 1, name, 'AGREE' if agree else 'DIFFER',
                          ' '.join('%s=%s/%s' % (side, (value or ('absent', 0))[0][:12], (value or ('absent', 0))[1])
                                   for side, value in values.items()))
    print('disagreements', disagreements)
    return 1 if disagreements else 0


if __name__ == '__main__':
    sys.exit(main())
```
