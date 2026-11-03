# Stage 2 mapping: original-byte consumers

The inventory was made against #1571 `fc55a45e` before B transferred the
mapping. These are the consumers of the retained original and their answers
in B. Library type comes from the `table.conf` system rows, not a fixed
index range; [the high-index case](1549-high-system-library.md) records the
pre-existing difference separately.

| Consumer | Answer after transfer |
| --- | --- |
| `SystemOriginals::{get,iter,selected,append,publish_current}`, paths and fault/loaded state | Library-level path, loaded state, original total and original range remain metadata. The system item bytes are moved into the session's live sub-index. An addon owns its one mapping directly. A layout-failed sub-index retains its mapping with empty item and offset views, as `phrase_index.cpp:335-360` does. |
| `LiveSystemItems::{read,pronunciation_count}`, runtime lookup, trellis, C getters, prediction, phrase and bigram export | Read the live mapped bytes or grown content under the session guard. The unloaded mask and the live read distinguish unloaded and absent items. |
| `SystemItems::is_removed`, `UserStore::system_item_removed`, `with_appended_item`, import classification and the visibility-filtered prefix probe | After the first write, a packed load-time presence bitmap distinguishes a removed original from an always-absent or appended slot, and preserves the prefix probe's original-presence answer. Before the first write the live buffer is the baseline; consumers use it or know no appended/removal state exists. Current presence comes from the live buffer. |
| `UserStore::unigram_delta`, `LiveSystemItems::stored_unigram`, replay seeding and original item total | After the first write, a `u32` per slot supplies the original unigram for delta and baseline consumers; before the first write the delta is zero and the live field is the original. The original sum and range are scalars. Readability equals presence because the complete item view bounds-checks all phrase words. Prediction and current unigram queries read the live field directly. |
| `SystemItems` totals, compaction, import, training and range | Read and write the live buffer. The sub-index and facade totals retain their independent arithmetic; overlapping training moves neither total. The live range includes appended items. |
| `Merge`, replay finalization and row 107 | Transfer the sole mapping before replay. Writes use live offsets and content; an undo vector records constant-sized typed entries with old content borrowed from the journal. Only a reached payload over-read reverses entries. Finalization uses a temporary touched-slot bitmap and does not retain decoded original items. A mismatch or unknown record keeps the applied prefix. |
| `Runtime::read_save_baseline`, `diff_live_records`, direct-store save | Map the current system file at the save point and diff it against the live sub-index. The temporary mapping is dropped after the diff. Row 76's stop and retry stay in force. |
| Reload, unload, `mask_out` and visibility | A successful remap transfers its new owner into the live sub-index; unload drops the live owner. The row 76 mask preflight keeps every pre-call live buffer when any remap fails. Paths and load-time metadata are replaced with the library. |
| Value-only and test-facing exports | Value-only callers use their explicit fixture or fresh save baseline. Session item reads use the live owner. No original byte mapping is retained for tests. |

The original B history used `6S` logical bytes: two one-byte-per-element
`Vec<bool>` arrays and one `u32` per slot. History is now allocated only on
first write: `4S + 8 ceil(S/64)` logical bytes plus allocation overhead for
unigrams and a packed presence bitmap. Transfer state is separate. The first
write pays an `O(S)` scan; a first item-count query may independently scan `S`
slots and cache a scalar. Replay finalization's temporary `Vec<bool>` uses one
byte per live slot, plus allocation overhead, and its undo entries are
separate from history.
