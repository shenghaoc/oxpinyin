# Roadmap

The [source policy](docs/source-policy.md#stages) defines what each stage permits.

## Reference pin

The pin is libpinyin 2.11.92 at `074a2219`.
[`tools/oracle/oracle-pin.txt`](tools/oracle/oracle-pin.txt) is authoritative.
The reference cell is tkrzw on both sides.
The [pinned-behaviour page](docs/pinned-behaviour.md) defines the build and the behaviour target.

## Stage 1: complete

The candidate surface agrees bit-identically with the pin-built oracle on every corpus input.
The drop-in library passes all 1,571 corpus rows and exports all 79 `pinyin_*` symbols.
The sentence-trellis residual is a frozen exception (compatibility policy row 11).

## Stage 2: current

Stage 2 is bug-for-bug compatibility with the pin, with compatible internal improvement.
The round-2 audit (#573) and the init complexity tracker (#870) are closed.

### Exit gate: the third bug-for-bug sweep

- Run every differential on all three cells.
- Check every divergence-register row against the pin's source.
- Include the round-2 audit items carried to round 3 (#573).
- Record language artefacts under the [internal-language-artefact rule](docs/findings/compatibility-policy.md#internal-language-artefacts); do not reproduce them.

### Open items in the algorithm record

The [algorithm record](docs/findings/libpinyin-structures-and-algorithms.md) lists these gaps.
They do not block the sweep.

- Write the §11 equivalence arguments.
  Record each failed argument as a separate defect.
- Argue each row of §§1–8 that is marked different or unverified.
  Compare its time and space complexity with the pin.
- Complete the §9 inventory of coefficients and limits on the parity path, each with its pin value.
- Verify what the source does not settle for sentence scoring (§11, "Sentence scoring: gfloat and libm"): multiply-add contraction in the pin's `libpinyin.so`, and the `log` that `f64::ln` resolves to in the shipping build.
- Run the constraint-revalidation acceptance case (§11): a pinned-model phrase whose abbreviation expands past 4096 keys.
  Force it, then revalidate through `guess_sentence` and a replaced selection.
  Compare with the reference-cell oracle using a contract test.
- Replace the `SEARCH_*` and `ERROR_*` integer codes in `oxpinyin-user/src/phrase_table.rs` with an enum or `Result`.

### Compatible internal improvement

These changes keep output, files and the ABI unchanged.
Each lands on its own when ready and does not block the sweep.

- Dictionary cursor walks for lattice building.
- Incremental lattices for each keystroke.
- Per-session arenas.
- No ordered atomics on the steady cycle.
- Host-independent cost derivation with identical output on every cell.

## Stage 3: after Stage 2

Nothing has landed.
The [source policy](docs/source-policy.md#stage-3) states when Stage 3 starts and what a departure requires.
Each candidate adds one opt-in cargo feature:

1. Compile the text model during data preparation.
   This changes the system data files.
2. Move the system bigram from the backend's `bigram.db` into a compiled blob.
   This changes the system data files.
3. Upgrade the model: trigram or Kneser-Ney smoothing, typo edges, or oxpinyin's own data.
   This changes output.
