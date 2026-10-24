# 894-non-scalar-phrase-text — non-scalar stored phrase text

- **Status:** accepted under the existing category; verification pending · 2026-10-24 UTC · issue #894.
- **Disposition:** POLICY-ACCEPTED (never MATCH).
- **Policy limb:** language/runtime quirk, decision-order limb 3. This is
  not transcendental math (a), an invalid memory access (b), or a pin
  assertion (c). Safe byte storage could reproduce GLib's output; Rust's
  `str` restriction alone is not a class-(b) argument.
- **Pin:** libpinyin 2.11.92, `074a2219c90feaf962d0d24f034514033ece5f99`,
  read from `/workspace/libpinyin-pin` (clean detached checkout).
  Both facades; all DBM backends. Reference image GLib **2.89.4-2**,
  not the issue's 2.80.0. The snapshot `20260831T000000Z` Debian source
  package was read, including its patch series: none changes `gutf8.c`.
  Its upstream `gutf8.c` equals the GNOME 2.89.4 tag at
  `fa41d356ee4936264c45cf11fa6c2640a89fbdda`.
- **Pin derivation:** GLib `glib/gutf8.c:1035-1047` stops on zero and
  rejects only values >= `0x80000000`; `:1054-1060` encodes with
  `g_unichar_to_utf8` (`:549-607`). U+D800 becomes `ed a0 80` and
  U+110000 becomes `f4 90 80 80`. Neither is valid UTF-8; both are
  non-NULL allocated results. The issue's semantic claim survives the
  version correction; its line numbers do not.
- **Rust counterpart:** `oxpinyin-data/src/phrase_library.rs`,
  `PhraseItemView::{phrase_chars,phrase_text,phrase_text_into}` uses
  `char::from_u32`. A single non-scalar makes collection return `None`.
  `phrase_libraries.rs`, `phrase_char_count` and `trellis_item` refuse it
  too; `dict.rs`, `resolve_item` marks a loaded but undecodable item as
  unreadable. `oxpinyin-runtime/src/lib.rs`'s dictionary implementations
  and user overrides preserve the same scalar check.

## Forward trace

1. Both `*_init` and library loaders read the native phrase chunks;
   `PhraseItem::get_phrase_string` copies the stored words without Unicode
   validation (`src/storage/phrase_index.cpp:103-111`). Phonetic parsing
   searches separate packed-key indexes; malformed *text* need not make
   the pronunciation or token unreadable.
2. Candidates: pinyin `src/pinyin.cpp:1953-2038` obtains the stored length
   and converts normal, longer, predicted and addon items through
   `_token_get_phrase` (`:1627-1645`); zhuyin has the corresponding
   `src/zhuyin.cpp:1096-1113,1294-1344`. Sorting uses the stored length and
   counts, not UTF-8 validation; dedup uses `strcmp`
   (`pinyin.cpp:2041-2138`, `zhuyin.cpp:1348-1416`). The pin exposes the
   invalid bytes as a borrowed candidate string. In Rust, `resolve_item`
   rejects the phrase; both C facades' `sentence.rs` candidate builders
   detect `had_unreadable_item`, clear the entire window, return false and
   emit one domain warning. This existing warning is an observation, not
   a newly claimed pin crash or class-(b) site. Prediction's text-filtering
   paths can instead omit just the undecodable item.
3. Sentences: `PhoneticLookup` prices stored length and pronunciation
   without converting text (`src/lookup/phonetic_lookup.h:643-695`). The
   selected token path becomes bytes in `src/lookup/lookup.cpp:27-69`;
   `pinyin_get_sentence` (`pinyin.cpp:1464-1481`) and `zhuyin_get_sentence`
   (`zhuyin.cpp:983-998`) expose it with `g_free` ownership. Rust's
   `PhraseLibraries::trellis_item` excludes the non-scalar token before
   pricing; `nbest.rs`, `token_text`/`text`, also refuses a missing text
   and never concatenates an undecodable row. With the isolated witness
   path there is no complete sentence, so guess/get return false and get
   leaves the caller's output untouched. With alternate valid paths the
   best surviving sentence can change, still only on the malformed table.
4. Export: `pinyin_begin_get_phrases` probes item/pronunciation presence
   (`pinyin.cpp:662-688`), then `pinyin_iterator_get_next_phrase`
   (`:698-768`) converts text and returns the reading/count, advancing the
   cursor normally. Rust's `PhraseExportCursor`/`ExportRow::text_len` in
   `oxpinyin-facade/src/export_rows.rs` keeps the row but yields no text;
   `oxpinyin-capi/src/iterators.rs` returns true, NULL phrase, and intact
   reading/count. Policy row 62's old claim that GLib returns NULL for
   every non-scalar is incorrect. GLib rejects >= `0x80000000`, not
   surrogates or U+110000. **libzhuyin has no export iterator**:
   `src/zhuyin.h`/`src/libzhuyin.ver` expose import only. Its export cases
   check this absence; they cannot call a fabricated twin.

## Backward trace and valid-input proof

Every text producer reaching those consumers is one of:

- A system/addon chunk, a saved user chunk or system override journal.
  The loaders preserve their native representation. Replaying a corrupt
  journal can propagate corrupt text; it cannot make that input valid.
  Backend selection changes the KV reader, not these chunk words or
  conversion functions. The addon candidate path uses the same decoder;
  promotion copies its item (`pinyin.cpp`'s addon selection path), so a
  malformed addon can contaminate a user item without losing its origin.
- Import through `pinyin_iterator_add_phrase` (`pinyin.cpp:614-652`) or
  `zhuyin_iterator_add_phrase` (`zhuyin.cpp:500-537`), and pinyin remember
  (`pinyin.cpp:3669-3710`, `_remember_phrase_recur` -> `_add_phrase`). They
  obtain words from NUL-terminated UTF-8. Valid UTF-8 encodes only scalar
  values. **Malformed C strings can also reach this in the pin**:
  GLib `g_utf8_to_ucs4` (`gutf8.c:940-987`) accepts some non-scalar byte
  sequences. Thus the issue's unrestricted claim that C inputs cannot
  produce these words is too strong; the correct claim is that *valid
  UTF-8 C inputs* cannot. Rust's strict/lossy import conversion cannot
  produce non-scalars. This is the adjacent #918 input boundary, not a
  valid-input escape or a reason to weaken this record.
- System-table generation uses `FacadePhraseIndex::load_text`
  (`phrase_index.cpp:522-555`), again NUL-terminated UTF-8. Normal table
  text produces scalars; invalid source text produces a malformed table.
- Training updates counts/readings, selection retains tokens, and save
  serializes items; none creates a non-scalar from scalar phrase text.
  Prefix lookup, phrase segmentation and `lookup_tokens` select tokens;
  their output text still comes from the item, not an unchecked new text
  producer. Direct `*_token_get_phrase` reaches the same conversion:
  pin returns true/declared length/invalid bytes; Rust returns false with
  outputs untouched when introspection cannot decode the item.
- NBEST candidates consume the sentence path above; punctuation strings
  are a separate producer (#1144), not admitted by this record. Bigram
  export converts both items (`pinyin.cpp:822-830`) but also has the
  independent unterminated-vector class-(b) defect (register row 1);
  this record does not accept that fault by analogy. Rust's materialized
  `ContextCore::export_phrases` fails on an absent text rather than
  producing a partial dictionary; its Rust-only result is not a C ABI.

**Input partition:** valid supported tables have bounded, complete items,
valid readings and nonzero Unicode scalar phrase text. A malformed table
with U+D800 or U+110000 in an otherwise structurally valid item is the
witness. Values >= `0x80000000`, truncated items, invalid pronunciations,
allocation failure and independent crash sites are outside these expected
observations. Empty, overlong and inconsistent items keep their existing
rules. A valid query against a malformed table is still malformed state.

**Why accepted:** the difference is GLib's historical extended UTF-8
encoding versus ordinary Rust scalar decoding, confined to the proved
malformed partition. No application rule on valid text is changed.
**Contract preserved:** C symbols/layout, borrowed candidate lifetimes,
caller-owned `g_free` strings, real `GArray` ownership, native on-disk
formats and data ownership stay unchanged. No process-wide side effect
changes. Candidate warnings and output/return differences above are
explicitly recorded, not suppressed. No production behavior changes.

## Tests and methodology

Existing driver: `tools/bisection/contract-diff.py --issue 894`. Cases
`phrase-text/{pinyin,zhuyin}-{surrogate,above-unicode}-{candidate,sentence,export}`
use a private system chunk and native index; they retain raw byte hex,
returns, output sentinels and logs. Pinyin paths and both-facade candidate/
sentence paths are POLICY-ACCEPTED; absent zhuyin export is MATCH.
`phrase-text/*-valid-*` are ordinary scalar controls, MATCH. The current
record's machine-readable observations are binding; an unexpected result
on either side fails, even if both sides happen to agree. Driver tests
cover missing/stale records and missing/wrong observations.

Source was read before provisioning or executing any differential. Debian
source came from `snapshot.debian.org/archive/debian/20260831T000000Z/pool/main/g/glib2.0/`;
the `.dsc` SHA-256 values were checked for the orig and Debian tarballs.
No runtime experiment supplied the derivation. Runtime results and the
named subject mutation will be reported after the gate; captures remain
ephemeral. **Prior rulings:** none reversed. **Conclusion:** retain normal
Rust behavior under limb 3; source classification complete, verification
pending (2026-10-24 UTC).
