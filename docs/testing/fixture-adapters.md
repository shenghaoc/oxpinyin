# W4 fixture data seam

Date: 2026-08-09 · Status: **frozen for W4-T0b**

This page records the fixture doubles' data, provenance and limits.

## Location (updated 2026-11-06 UTC)

The fixture doubles are one source file,
`crates/oxpinyin-core/tests/common/fixture_doubles.rs`. Each test target that
needs them includes it with `#[path]`, so the production build graph carries no
test doubles and no shared test crate can close a dependency cycle through
core. The provenance, data, and behavior are unchanged. See
`docs/testing/upstream-test-coverage.md` for the wider test-architecture
reorganization.

## Why not the real thing

Portable decoder tests use authored fixture doubles. Model redistribution follows
the scoped [provenance ruling](../findings/model-provenance.md).

## What F-B would have been, and why this is not it

`spec-derivation` reserves family **F-B** for "corpus utterances →
candidate lists to depth 10", feeding W4 acceptance. F-B was never captured;
Phase 0 produced F-A and F-C only. This fixture is the stand-in, and it is
narrower than F-B in a way worth stating plainly: it is a *vocabulary*, not a
capture family, and it carries no parity claim.

Capturing F-B properly would still not change the weights. The pinned oracle
exposes candidate *order* at its public API, never a probability, and its
probabilities are the non-redistributable artefact. So even with F-B in hand,
the weights below would remain authored.

## Provenance, part by part

| Part | Source | Status |
|---|---|---|
| Phrase text | `fixtures/foundation/f-a.txt`, `f-c.txt` candidate columns | captured pinned-oracle output |
| Key sequences | authored | authored |
| Order within a key sequence | captured candidate rank | derived |
| Unigram weights | `1000 − 100 × best captured rank` | derived |
| Bigram counts | authored | authored |

### Why key sequences cannot be derived

The obvious mechanical rule — assign a candidate to the first *n* keys of the
oracle's selected path — is wrong, and the capture says so. For input
`fangan` the pin selects `fan@0:3, gan@3:6`, yet its candidate list opens

```text
方案  反感  方  房  放  防  芳  坊  访  仿
```

`方案` is `fang` + `an` and `方` is `fang`, neither of which is on the selected
path. The pin ranks candidates across **every** segmentation the graph admits,
not along one path. The same happens for `xian`, whose first three candidates
(`西安`, `西岸`, `锡安`) are `xi` + `an`.

That is a useful fact for W4-T1 rather than an obstacle: it is direct evidence
that candidates come from a graph. But it means a candidate list cannot tell
us which keys a candidate covers, so the key sequences here are authored from
ordinary pinyin and are not claimed as captured.

### Why weights follow rank

The one thing the capture does state about relative likelihood is the order it
lists candidates in. `1000 − 100 × rank` turns each observed rank into a
weight while preserving that order exactly, across the whole fixture rather
than per input, and it is reproducible from the committed captures. A phrase
observed at several ranks takes its best.

`crates/oxpinyin-core/tests/fixture_provenance.rs` enforces all of this: every
phrase must appear in a committed capture, every weight must equal the rule's
output, and entries sharing a key sequence must stay in captured order. The
fixture cannot silently drift away from the pin's observed ranking.

### Why bigrams are authored

Nothing observable supplies them. They exist to exercise the interpolation
arithmetic and to make a two-phrase path score differently from a one-phrase
path. `fixtures/w4/mini-bigram.txt` says so in its own header.

## Format

Line-oriented UTF-8, one record per line, TAB-separated `key=value` fields —
the same shape as `pinyin-capture-v1`, so the same eye and the same tooling
read both. A blank line, a line starting with `#`, and a trailing field
starting with `#` are comments. Values carry no TAB and no newline, so no
escaping layer is needed.

```text
token=1	keys=ni	text=你	unigram=1000
prev=11	next=33	count=800	# 你好 -> 中国
```

`token` is dense and numbers records `1..=n` in file order. `keys` is a
comma-separated list of frozen pinyin keys. A bigram naming an undefined token
is an error, not a silent skip.

## Adapters

`fixture_doubles.rs` provides:

- `FixtureDictionary` — `Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>`.
  Lookup is exact on the whole key slice and returns entries in fixture order.
  An absent sequence is `Ok(vec![])`, never an error.
- `FixtureLanguageModel` — `LanguageModel<Token = PhraseToken>`, an
  interpolated bigram over the same vocabulary, combining the caller's edge
  cost as the frozen seam requires.

Both parse from `&str`. The doubles do no I/O; callers `include_str!`.

`oxpinyin_core::cost` holds the arithmetic: costs are integers on a fixed-point
negative-log₂ scale, one bit of surprisal per `COST_PER_BIT`, with an
`UNKNOWN_COST` floor for events the model gives no mass. **No floating point.**
`f64::ln` is not required to be bit-identical across platforms and libms, and
constitution item 6 makes output a pure function of (input, user state,
config) on every operating system. The fixed-point logarithm is integer
squaring and shifting only.

The interpolation weight here is one half — an authored, deliberately neutral
value. The real model's λ comes from `table.conf`
([scoring](../findings/scoring-spec.md#the-interpolated-bigram)); nothing in
this module claims to be one of the model's constants.

## Replacing this with real data

W3's loaders implement the same two traits over the compiled-in store
backend's tables. The
integration is a change of two type arguments at the `Session::new` call site.
Nothing in `oxpinyin-core`'s decoder, and nothing in `oxpinyin-engine`'s session,
mentions either implementation.

## Acceptance

- Both adapters implement their frozen trait, return `Result`, and never
  panic on malformed fixture text.
- Lookups and scores are deterministic.
- Portable: no platform code, no I/O, no dependency.
- Provenance is test-enforced, not asserted in prose.
