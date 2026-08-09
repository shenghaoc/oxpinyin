# Findings — pinned oracle aborts on apostrophe-only input

Date: 2026-08-09 · Source tier: Architect observation from W2-T3.
Status: **registered as F-E-14.** Harness guard accepted (maintainer decision
2026-08-09). Upstream issue against the pinned tag is for the maintainer to
file — do not open it from this finding.

## Summary

On the pinned oracle, an input consisting only of ASCII apostrophes makes
`pinyin_get_pinyin_key` abort the process:

```text
Assertion `index < m_table_content->len' failed.
../src/storage/phonetic_key_matrix.h:103
size_t pinyin::PhoneticTable<Item>::get_column_size(size_t) const
```

This is `assert()` reaching `abort()`, so it is not recoverable in-process. It
is the same class as catalogue row F-E-12 (issue #542, assertion on user
input), on a different code path and a different input shape. Registered as
its own row **F-E-14** rather than folded into F-E-12, because the trigger
and the upstream file differ.

A lone apostrophe is ordinary user input. It is what a user sees mid-word while
typing `xi'an`. The one-character repro for the upstream report is:

```text
lone apostrophe "'"  →  assert()  →  abort()
```

## Reproduction

Pin: `libpinyin-2.11.91-0c5e80e1200f84fab185d1c5bde458b770a0636c+model20-59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155+dbm-tkrzw`,
built by `tools/oracle/build-oracle.sh`. Flags `0x18a` (the F-A profile).

Sequence: `pinyin_init` → `pinyin_set_options` → `pinyin_alloc_instance` →
`pinyin_parse_more_full_pinyins(instance, "'")` →
`pinyin_get_parsed_input_length` → `pinyin_get_pinyin_key(instance, 0)`.

The last call aborts.

## Measured cause

`pinyin_parse_more_full_pinyins` and `pinyin_get_parsed_input_length` were
called *without* any key query, to observe the reported state before the abort:

| Input | Bytes | `parse_return` | `parsed_input_length` |
|---|---:|---:|---:|
| `'` | 1 | 1 | 1 |
| `''` | 2 | 2 | 2 |
| `'''` | 3 | 3 | 3 |
| `ni'` | 3 | 3 | 3 |
| `'ni` | 3 | 3 | 3 |
| `a'a` | 3 | 3 | 3 |
| `!` | 1 | 0 | 0 |
| `q` | 1 | 1 | 1 |

The oracle reports a **non-empty parsed prefix** for apostrophe-only input while
its phonetic key matrix has **no columns**. Any key query at offset 0 is then
out of range, and the assertion fires.

Contrast `!`, which reports `parsed_input_length = 0`: no walk is attempted, so
nothing aborts. The defect is specifically the inconsistency between a
non-zero parsed length and an empty key matrix.

## Scope of the abort

Verified one input per process so an abort could not mask later results:

| Input | Result |
|---|---|
| `'`, `''`, `'''` | **abort** |
| `ni'` | survives: `ni@0:2:complete,<missing-pinyin>@2` |
| `ni'!` | survives: `ni@0:2:complete,<missing-pinyin>@2` |
| `'ni` | survives: `ni@1:3:complete` |
| `a'a` | survives: `a@0:1:complete,a@2:3:complete` |
| `ni''hao` | survives: `ni@0:2:complete,hao@4:7:complete` |
| `ni'h` | survives: `ni@0:2:complete,h@3:4:partial` |
| `xi'an` | survives: `xi@0:2:complete,an@3:5:complete` |

So the abort requires the parsed prefix to contain **no** syllable-bearing byte.
Once any lowercase letter is present, a key column exists and the walk is safe.

Two side observations worth registering separately:

- `ni'` and `ni'!` reproduce the **F-E-01 shape live**: a key column with no
  usable pinyin string, which the harness records as `<missing-pinyin>`. Until
  now that shape was modelled but unobserved, since no F-A case triggers it.
- `ni''hao` shows the oracle consuming all 7 bytes across a doubled
  apostrophe, where `docs/findings/parser-path-set.md` freezes our parser to
  stop at `ni` with remainder `''hao`. That is a genuine apostrophe-policy
  difference, not a bug on either side, and it is what the divergence taxonomy
  should classify.

## Guard adopted in W2-T3 — accepted

The harness must survive the corpus, and an `abort()` cannot be caught, so the
key walk is skipped exactly when it would be unsafe:

> If `parsed_input_length > 0` and the parsed prefix contains no ASCII lowercase
> letter, emit no syllable segments and record the sentinel
> `<no-key-columns>@0` instead of querying keys.

The guard is sound rather than merely empirical: a phonetic key is built from an
initial and a final, both spelled with lowercase ASCII letters, so a parsed
prefix with no letter cannot have produced a key. The condition is therefore
"the oracle claims to have parsed something that cannot contain a key".

The guard **does not hide the defect**. `<no-key-columns>` is a sentinel, so
`OracleObservation::has_sentinel_segment` is true and the differential runner
reports the input as a divergence with reason `oracle-sentinel`. The affected
inputs appear in the divergence log rather than silently passing.

Under the parity corpus the guard fires on exactly the three apostrophe-only
inputs in `09-edge.txt`.

**Maintainer decision:** the guard's placement in the harness is accepted. The
alternative — excluding apostrophe-only inputs from the corpus — would hide a
real robustness defect and is rejected.

## What this does not do

It does not patch the oracle, and it must not. The oracle is the parity subject
at a fixed pin; changing it would invalidate every fixture. The guard lives
entirely in our harness.

## Upstream report (maintainer action)

File upstream against the **pinned tag**
(`2.11.91` / commit `0c5e80e1200f84fab185d1c5bde458b770a0636c`) with the
one-character repro: lone apostrophe `'` → `assert()` → `abort()` via
`pinyin_get_pinyin_key` after `pinyin_parse_more_full_pinyins`.

**Do not file the upstream issue from agent work.** The maintainer will file
it. This finding only records that the report is warranted and what the repro
is.

## Maintainer decisions (2026-08-09)

1. **Register as F-E-14** (own row; not folded into F-E-12).
2. **Upstream report** — yes, against the pinned tag; maintainer files it.
3. **Guard in the harness** — accepted; keep apostrophe-only inputs in the
   corpus.
