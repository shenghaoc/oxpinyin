# W4 fixture data seam

Status: **current reference**

These test doubles are internal test infrastructure, not a compatibility contract ([external interfaces: C only](../findings/compatibility-policy.md#external-interfaces-c-only)).

This page records the fixture doubles' data, provenance and limits.

## Location

The fixture doubles are one source file,
`crates/oxpinyin-core/tests/common/fixture_doubles.rs`. Each test target that
needs them includes the file with `#[path]`. The production build graph therefore carries no
test doubles, and no shared test crate can close a dependency cycle through
core. For the wider test organization, see the
[upstream test coverage ledger](upstream-test-coverage.md).

## Why not the real thing

Portable decoder tests use authored fixture doubles. Model redistribution follows
the scoped [provenance ruling](../findings/model-provenance.md).

## Not a capture family

The fixture is a *vocabulary*, not a capture family, and it carries no parity claim.
A captured family of corpus utterances and their candidate lists would not change the weights:
the pinned oracle exposes candidate *order* at its public API, never a probability, and its
probabilities are the non-redistributable artefact. The weights below would therefore still be authored.

## Provenance, part by part

| Part | Source | Status |
|---|---|---|
| Phrase text | `fixtures/foundation/f-a.txt`, `f-c.txt` candidate columns | captured pinned-oracle output |
| Key sequences | authored | authored |
| Order within a key sequence | captured candidate rank | derived |
| Unigram weights | `1000 − 100 × best captured rank` | derived |
| Bigram counts | authored | authored |

### Why key sequences cannot be derived

One mechanical rule assigns a candidate to the first *n* keys of the
selected path of the oracle. That rule is wrong, and the capture shows it. For input
`fangan`, the pin selects `fan@0:3, gan@3:6`, but its candidate list starts with

```text
方案  反感  方  房  放  防  芳  坊  访  仿
```

`方案` is `fang` + `an` and `方` is `fang`. Neither is on the selected
path. The pin ranks candidates across **every** segmentation that the graph admits,
not along one path. The same happens for `xian`: its first three candidates
(`西安`, `西岸`, `锡安`) are `xi` + `an`.

A candidate list therefore cannot show which keys a candidate covers. The key sequences here are
authored from ordinary pinyin and are not claimed as captured.

### Why weights follow rank

The only statement that the capture makes about relative likelihood is the order in which it
lists candidates. `1000 − 100 × rank` turns each observed rank into a
weight and keeps that order exactly, across the whole fixture and not
per input. The committed captures reproduce it. A phrase
observed at several ranks takes its best rank.

`crates/oxpinyin-core/tests/fixture_provenance.rs` enforces these rules:

- every phrase must appear in a committed capture;
- every weight must equal the output of the rule;
- entries that share a key sequence must stay in captured order.

The fixture therefore cannot drift away from the observed ranking of the pin without a test failure.

### Why bigrams are authored

Nothing observable supplies them. They exist to exercise the interpolation
arithmetic, and to make a two-phrase path score differently from a one-phrase
path. The header of `fixtures/w4/mini-bigram.txt` states this.

## Format

The format is line-oriented UTF-8, one record per line, with TAB-separated `key=value` fields.
It has the same shape as `pinyin-capture-v1`, so the same reader and the same tooling
read both. A blank line, a line that starts with `#`, and a trailing field that
starts with `#` are comments. Values carry no TAB and no newline, so no
escaping layer is necessary.

```text
token=1	keys=ni	text=你	unigram=1000
prev=11	next=33	count=800	# 你好 -> 中国
```

`token` is dense and numbers records `1..=n` in file order. `keys` is a
comma-separated list of frozen pinyin keys. A bigram that names an undefined token
is an error, not a silent skip.

## Adapters

`fixture_doubles.rs` provides:

- `FixtureDictionary` — `Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>`.
  Lookup is exact on the whole key slice and returns entries in fixture order.
  An absent sequence is `Ok(vec![])`, never an error.
- `FixtureLanguageModel` — `LanguageModel<Token = PhraseToken>`, an
  in-memory unigram model over the same vocabulary. It checks every bigram
  record against the vocabulary. `FrequencyFixtureModel` wraps it and answers
  the unigram queries.

Both parse from `&str`, return `Result` and never panic on malformed fixture text.
Lookups and scores are deterministic. The doubles do no I/O and have no platform code
and no dependency; callers use `include_str!`.

The interpolation weight here is one half: an authored, deliberately neutral
value. The λ of the real model comes from `table.conf`
([scoring](../findings/scoring-spec.md#the-interpolated-bigram)). Nothing in
this module claims to be one of the constants of the model.
