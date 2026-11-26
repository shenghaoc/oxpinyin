"""The mini model20 subset behind `fixtures/w3` and the pin's build sequence over it. Standard library only.

`fixtures/w3/<ext>` is one system data directory per storage backend: the
pin's own three-program sequence (`data/Makefile.am:60-62` at 074a2219),
`gen_binary_files --gen-punct-table`, `import_interpolation` and
`gen_unigram`, run over a subset of the pinned model20 text, then
`gen_deleted_ngram` over a fixed two-token corpus. The subset keeps the
phrases the `MINI_KEYS` and `MINI_ART_KEYS` spellings name, their real
`\\1-gram` counts, and the `\\2-gram` rows whose both tokens survive; the
programs recompute every bigram total from the surviving rows.

The committed sets are frozen; nothing here writes or checks them. This
module supplies `tools/oracle/pin_capture.py` with the subset (`subset`),
the `table.conf` renderings (`table_conf`), the pin's sequence
(`sequence`) and the verified model20 extraction (`model_dir`): the
archive is not redistributable, so `model_dir` takes the extracted
directory (`--model-dir`, `$PINYIN_MODEL_DIR`, or `just fetch-model`'s
cache) and refuses one whose cache lacks the fetch's verified marker.
"""
import os
from pathlib import Path
import shutil

from . import model

ROOT = Path(__file__).resolve().parents[2]

# The spellings whose phrases the mini set keeps: single syllables, two
# multi-syllable phrases, and the `xian` / `xi'an` pair whose distinctness
# is what the apostrophe-separated key format exists to preserve.
MINI_KEYS = ("a", "ni", "hao", "ni'hao", "ni'men", "zhong", "guo", "zhong'guo", "xian", "xi'an")
# The art.table spellings the addon pair and `art.bin` keep.
MINI_ART_KEYS = ("er'huang", "bo'cai", "ban'she")
# The four default-loaded libraries (table.conf's `default …_DICTIONARY`
# rows) and the one addon library the subset reaches.
SYSTEM_TABLES = ('gb_char', 'gbk_char', 'opengram', 'merged')
ADDON_TABLE = 'art'


class Refused(Exception):
    """A diagnostic to print and the status to exit with."""

    def __init__(self, message, status=1):
        super().__init__(message)
        self.status = status


# ── the subset ───────────────────────────────────────────────────────

def parse_table(path):
    """The well-formed rows of a `.table` file as `(line, pinyin, phrase, token, count)`.

    Four whitespace-separated fields; blank and `#` lines are skipped, a
    short, non-numeric or negative-count row is skipped as the pin's
    `table_text_read_pinyin_record` skips it, and a row with more than
    four fields is refused (no pinned row carries one).
    """
    rows = []
    for number, raw in enumerate(path.read_text(encoding='utf-8').splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith('#'):
            continue
        fields = line.split()
        if len(fields) > 4:
            raise Refused(f'{path}:{number}: more than four fields: {line}')
        if len(fields) < 4:
            continue
        try:
            token, count = int(fields[2]), int(fields[3])
        except ValueError:
            continue
        if count < 0:
            continue
        rows.append((raw, fields[0], fields[1], token, count))
    return rows


def kept_tokens(rows, keys):
    """Every token a row with a spelling in KEYS names."""
    return {token for _, pinyin, _, token, _ in rows if pinyin in keys}


def write_rows(path, rows, kept):
    """The rows of KEPT tokens, in file order, as the model wrote them."""
    path.write_text(''.join(line + '\n' for line, _, _, token, _ in rows if token in kept), encoding='utf-8', newline='\n')


def subset_interpolation(source, destination, kept):
    """Keep the `\\1-gram` items of KEPT tokens and the `\\2-gram` items with both tokens kept.

    Header, section and `\\end` lines pass through; `import_interpolation`
    sums the surviving `\\2-gram` counts into each row's total, so no total
    is carried here.
    """
    section = None
    out = []
    for raw in source.read_text(encoding='utf-8').splitlines():
        fields = raw.split()
        if fields[:1] == ['\\item']:
            if section == '1' and len(fields) == 5:
                keep = int(fields[1]) in kept
            elif section == '2' and len(fields) == 7:
                keep = int(fields[1]) in kept and int(fields[3]) in kept
            else:
                raise Refused(f'{source}: unexpected item line: {raw}')
            if not keep:
                continue
        elif fields == ['\\1-gram']:
            section = '1'
        elif fields == ['\\2-gram']:
            section = '2'
        out.append(raw)
    destination.write_text(''.join(line + '\n' for line in out), encoding='utf-8', newline='\n')


def subset(model_dir, work):
    """Write the mini model's text inputs under WORK; return the kept token sets."""
    model_dir, work = Path(model_dir), Path(work)
    work.mkdir(parents=True, exist_ok=True)
    tables = {name: parse_table(model_dir / f'{name}.table') for name in SYSTEM_TABLES}
    kept = set()
    for rows in tables.values():
        kept |= kept_tokens(rows, MINI_KEYS)
    for name, rows in tables.items():
        write_rows(work / f'{name}.table', rows, kept)
    art = parse_table(model_dir / f'{ADDON_TABLE}.table')
    kept_art = kept_tokens(art, MINI_ART_KEYS)
    write_rows(work / f'{ADDON_TABLE}.table', art, kept_art)
    subset_interpolation(model_dir / 'interpolation2.text', work / 'interpolation2.text', kept)
    shutil.copyfile(model_dir / 'punct.table', work / 'punct.table')
    return kept, kept_art


def table_conf(dbm_name, *, build=False):
    """The committed rendering for DBM_NAME, or the one the build runs with.

    The utilities open every addon row's `.table` and chunk; the subset has
    rows in art alone, so the build's `table.conf` lists that one addon
    library and the committed rendering keeps the pin's full list.
    """
    template = (ROOT / 'data/table.conf.in').read_text(encoding='utf-8')
    text = template.replace('@DATABASE_FORMAT@', dbm_name)
    if build:
        lines = [line for line in text.splitlines() if not line.startswith('addon ') or line.startswith('addon 4 ')]
        text = ''.join(line + '\n' for line in lines)
    return text


# ── the build sequence ───────────────────────────────────────────────

def sequence(work):
    """The pin's three-program sequence over WORK's text (`data/Makefile.am:60-62` at 074a2219), as
    `(program, arguments, stdin)`, each run from the output directory."""
    work = Path(work).resolve()
    return (('gen_binary_files', ['--gen-punct-table', '--table-dir', str(work)], b''),
            ('import_interpolation', ['--table-dir', str(work)], (work / 'interpolation2.text').read_bytes()),
            ('gen_unigram', ['--table-dir', str(work)], b''))


# ── the model directory ──────────────────────────────────────────────

def model_dir(value, environment=None):
    """The verified extracted model directory, or a refusal naming the fetch."""
    environment = os.environ if environment is None else environment
    path = Path(value or environment.get('PINYIN_MODEL_DIR') or model.default_cache(environment) / 'extracted').resolve()
    if not model.extraction_complete(path):
        raise Refused(f'fatal: {path} is not a complete model20 extraction '
                      f'({len(model.EXPECTED_MODEL_FILES)} files); run `just fetch-model` or pass --model-dir', 2)
    if not model.marker_matches(path.parent / 'verified'):
        raise Refused(f'fatal: {path.parent / "verified"} does not record the pinned archive digest; '
                      'the fixtures are deterministic against that archive only, so extract it with '
                      '`just fetch-model` (`--archive PATH` takes a local copy)', 2)
    return path

