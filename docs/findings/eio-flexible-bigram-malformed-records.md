# FlexibleBigram malformed records (class (b), 2026-11-01 UTC)

**Class (b), registered by name.** The pin has three `FlexibleBigram`
implementations (`src/storage/flexible_ngram_bdb.h:165-187`,
`flexible_ngram_kyotodb.h:192-216`, `flexible_ngram_tkrzwdb.h:200-225` at
074a2219) hand a whole stored value to `FlexibleSingleGram`, which treats
the first `sizeof(ArrayHeader)` bytes as a header and every following
`sizeof(ArrayItemWithToken)` chunk as an item. A value shorter than one
header, or a value whose tail is not a whole number of items, is read
past its end when the pin later asks for its header or an item, or walks
its items.

Only a malformed or corrupted container reaches these paths. A container
written by either implementation has whole headers and items. No
tool-diff case plants such a file, so the behaviour is recorded here
rather than bound to a case.

## Records shorter than the array header

Updated 2026-11-09 UTC (#1760). For such a value,
`FlexibleBigram::get_array_header` and `FlexibleBigram::load`
(`crates/oxpinyin-store/src/flexible_ngram.rs`) return
`StoreError::ShortArrayHeader`. The K-mixture tools' `Database`
(`crates/oxpinyin-kmm/src/database.rs`) then follows the pin at each site.

**The pin asserts: the tools abort.** `FlexibleBigram::get_array_header`
asserts the header's length (`flexible_ngram_bdb.h:388`,
`flexible_ngram_kyotodb.h:371`, `flexible_ngram_tkrzwdb.h:388`). Its read
writes nothing first. oxpinyin calls `std::process::abort()` there, after
the pin's earlier work, under the constitution's abort rule. The sites are
`gen_k_mixture_model.cpp:295`, `estimate_k_mixture_model.cpp:77`,
`export_k_mixture_model.cpp:58`, `merge_k_mixture_model.cpp:155-156`,
`prune_k_mixture_model.cpp:165,183` and `validate_k_mixture_model.cpp:58`.
The missing glibc assertion line is POLICY-ACCEPTED.

**The pin reads outside the record: the tools report a store error.**
The pin's `load` succeeds on a short value. The pin then reads outside it
through `FlexibleSingleGram::get_array_header`
(`flexible_single_gram.h:300-306`), `insert_array_item` (`:151-181`) or
`retrieve_all` (`:88-105`). At that point, oxpinyin takes the tools'
store-error exit (`oxpinyin_input::tool::exit_on_store_error`). The exit writes the line
`flexible bigram single gram is shorter than its array header` on stderr
and then calls `exit(1)`, after the pin's earlier work. The sites are:

- `gen_k_mixture_model.cpp:252`, read at `:220`;
- `estimate_k_mixture_model.cpp:127` and `:130`, read at `:134` and `:136`;
- `import_k_mixture_model.cpp:223`, read at `:233`;
- `prune_k_mixture_model.cpp:150`, read at `:51`.

The loads that export and validate check (`export_k_mixture_model.cpp:79`,
`validate_k_mixture_model.cpp:91`) are the same kind of site, but the
tools reach them only after the asserting header walk at `:58` has read
every row.

**Merge copies a source row that the target lacks.** The pin loads the
source row (`merge_k_mixture_model.cpp:142`), finds no target row (`:143`)
and stores the loaded gram (`:145`), which writes the bytes it read
unchanged. The merge therefore copies a short source row into the result
and continues. oxpinyin copies the stored value with
`FlexibleBigram::copy_single_gram`. When the target has the row, a short
row on either side aborts at the asserts at `:155-156`.

## Records with a partial item tail

A value at least one header long whose tail is not a whole number of
items stays outside #1760. `FlexibleBigram::load` returns a backend
error for it, and `Database::load` answers that the row is missing.
`get_array_header` reads its header as the pin does. As a result:

- `gen_k_mixture_model` and `import_k_mixture_model` start a fresh gram
  for the row and store it over the stored value;
- `estimate_k_mixture_model` treats a candidate row as missing;
- `merge_k_mixture_model` aborts on such a source row at its checked load
  (`:142`) and copies the source row over such a target row;
- the checked loads of `export_k_mixture_model`,
  `validate_k_mixture_model`, `prune_k_mixture_model` and the deleted
  model in `estimate_k_mixture_model` abort.

The pin reads these values outside their records when it walks or
searches their items.
