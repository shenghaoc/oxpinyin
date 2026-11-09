"""The committed `fixtures/w3` sets: regenerate them, or check them. Standard library only.

`fixtures/w3/<ext>` is one system data directory per storage backend: the
pin's own three-program sequence (`data/Makefile.am:60-62` at 074a2219),
`gen_binary_files --gen-punct-table`, `import_interpolation` and
`gen_unigram`, run over a subset of the pinned model20 text, then
`gen_deleted_ngram` over a fixed two-token corpus. The programs are
`oxpinyin-utils`' ports, built for the directory's backend. The subset
keeps the phrases the `MINI_KEYS` and `MINI_ART_KEYS` spellings name, their
real `\\1-gram` counts, and the `\\2-gram` rows whose both tokens survive;
the programs recompute every bigram total from the surviving rows.

`generate` writes the regenerated set into `fixtures/w3/<ext>`; `check`
regenerates into a temporary directory and compares it with the committed
tree: the chunk files and `table.conf` byte for byte, the DBM files record
by record (their container bytes are the writing library's own layout and
change with its version). `content` is the older recipe for the content
`.bin` files at `fixtures/w3/` itself, truncated copies of the oracle's
installed custom-content tables.

The model20 archive is not redistributable and CI does not have it: both
`generate` and `check` take the extracted directory (`--model-dir`,
`$PINYIN_MODEL_DIR`, or `just fetch-model`'s cache) and refuse one whose
cache lacks the fetch's verified marker.
"""
import argparse
import hashlib
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import sysconfig
import tempfile

from . import cell, model, slot

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / 'fixtures/w3'

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
# The corpus `gen_deleted_ngram` trains `deleted_bigram.db` over
# (`fixtures/w3/README.md`): one two-token sentence.
DELETED_CORPUS = '16777219 阿\n16777220 啊\n'
# The committed `table.conf`: the pin's `data/table.conf.in` with its
# `@DATABASE_FORMAT@` filled in for the backend.
TABLE_CONF = '''binary format version:7
model data version:14
lambda parameter:0.312699

source table format:pinyin
database format:{format}

default RESERVED NULL NULL NULL NOT_USED
default GB_DICTIONARY gb_char.table gb_char.bin gb_char.dbin SYSTEM_FILE
default GBK_DICTIONARY gbk_char.table gbk_char.bin gbk_char.dbin SYSTEM_FILE
default OPENGRAM_DICTIONARY opengram.table opengram.bin opengram.dbin SYSTEM_FILE
default MERGED_DICTIONARY merged.table merged.bin merged.dbin SYSTEM_FILE
default ADDON_DICTIONARY NULL NULL addon.bin USER_FILE
default NETWORK_DICTIONARY NULL NULL network.bin USER_FILE
default USER_DICTIONARY NULL NULL user.bin USER_FILE

addon 4 art.table art.bin NULL DICTIONARY
addon 5 culture.table culture.bin NULL DICTIONARY
addon 6 economy.table economy.bin NULL DICTIONARY
addon 7 geology.table geology.bin NULL DICTIONARY
addon 8 history.table history.bin NULL DICTIONARY

addon 9 life.table life.bin NULL DICTIONARY
addon 10 nature.table nature.bin NULL DICTIONARY
addon 11 people.table people.bin NULL DICTIONARY
addon 12 science.table science.bin NULL DICTIONARY
addon 13 society.table society.bin NULL DICTIONARY
addon 14 sport.table sport.bin NULL DICTIONARY
addon 15 technology.table technology.bin NULL DICTIONARY
'''
# The DBM files of a data directory and their container class; every
# other `.bin` is a per-library chunk, compared byte for byte.
TREES = ('pinyin_index.bin', 'phrase_index.bin', 'addon_pinyin_index.bin',
         'addon_phrase_index.bin', 'punct.bin')
HASHES = ('bigram.db', 'deleted_bigram.db')
# Pinned inputs of the KMM tool cases, produced by the pin's own
# `import_k_mixture_model` (`fixtures/w3/README.md`); the recipe neither
# writes nor checks them.
PINNED = ('k_mixture_model.db', 'k_mixture_model_zero.db', 'k_mixture_model_edom.db')
CELLS = cell.CELLS
UTILITIES = ('gen_binary_files', 'import_interpolation', 'gen_unigram', 'gen_deleted_ngram')
# The cells with a Windows build: only tkrzw has one (`just tkrzw-build`, the CI tkrzw action).
WINDOWS_CELLS = ('tkrzw',)


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
    text = TABLE_CONF.format(format=dbm_name)
    if build:
        lines = [line for line in text.splitlines() if not line.startswith('addon ') or line.startswith('addon 4 ')]
        text = ''.join(line + '\n' for line in lines)
    return text


# ── the build ────────────────────────────────────────────────────────

def settings_for(name, environment=None):
    """The cell's feature, extension, DBM name and target directory."""
    try:
        return cell.Settings.read(dict(os.environ if environment is None else environment, PINYIN_ORACLE_DBM=name), ROOT)
    except cell.CellError as error:
        raise Refused(str(error), error.status) from error


def build_tools(settings, out=sys.stderr):
    """Build the four utilities and the record reader for the cell; return their paths."""
    settings.claim()
    common = ['cargo', 'build', '--locked', '--manifest-path', str(ROOT / 'Cargo.toml'),
              '--no-default-features', '--features', settings.feature]
    bins = [part for name in UTILITIES for part in ('--bin', name)]
    for command in ([*common, '-p', 'oxpinyin-utils', *bins],
                    [*common, '-p', 'oxpinyin-store', '--example', 'raw_records']):
        if slot.run_command(command, env=settings.environment, cwd=ROOT, stdout=2):
            raise Refused(f'FAIL: building for the {settings.cell} cell: {" ".join(command)}')
    debug = settings.target / 'debug'
    suffix = sysconfig.get_config_var('EXE') or ''
    tools = {name: debug / (name + suffix) for name in UTILITIES}
    tools['raw_records'] = debug / 'examples' / ('raw_records' + suffix)
    for path in tools.values():
        if not path.is_file():
            raise Refused(f'FAIL: missing artifact {path}')
    return tools


def run_tool(path, args, cwd, environment, stdin=b''):
    """Run one utility from CWD, as the pin's Makefile does; refuse a failure with its stderr."""
    done = subprocess.run([str(path), *args], cwd=cwd, env=environment, input=stdin,
                          capture_output=True, check=False, timeout=300)
    if done.returncode:
        raise Refused(f'FAIL: {path.name} {" ".join(args)} exited {done.returncode}\n'
                      + done.stderr.decode(errors='replace'))
    return done.stdout


def sequence(work):
    """The pin's three-program sequence over WORK's text (`data/Makefile.am:60-62` at 074a2219), as
    `(program, arguments, stdin)`, each run from the output directory."""
    work = Path(work).resolve()
    return (('gen_binary_files', ['--gen-punct-table', '--table-dir', str(work)], b''),
            ('import_interpolation', ['--table-dir', str(work)], (work / 'interpolation2.text').read_bytes()),
            ('gen_unigram', ['--table-dir', str(work)], b''))


def produce(settings, tools, work, out, environment=None):
    """Run the pin's sequence over WORK's text into OUT, the way `data/Makefile.am` does."""
    out = Path(out)
    out.mkdir(parents=True, exist_ok=True)
    environment = dict(os.environ if environment is None else environment)
    (out / 'table.conf').write_text(table_conf(settings.dbm_name, build=True), encoding='utf-8', newline='\n')
    for program, arguments, stdin in sequence(work):
        run_tool(tools[program], arguments, out, environment, stdin=stdin)
    run_tool(tools['gen_deleted_ngram'], ['--deleted-bigram-file', 'deleted_bigram.db'], out, environment,
             stdin=DELETED_CORPUS.encode())
    (out / 'table.conf').write_text(table_conf(settings.dbm_name), encoding='utf-8', newline='\n')


# ── the comparison ───────────────────────────────────────────────────

def records(reader, kind, path, environment=None):
    """The DBM file's records as `<key hex> <value hex>` lines, in key order."""
    return run_tool(reader, [kind, str(path)], path.parent, dict(os.environ if environment is None else environment)
                    ).decode().splitlines()


def compare_dbm(reader, kind, produced, committed, environment=None):
    """`(identical, detail)` for one DBM file, by records."""
    ours, theirs = records(reader, kind, produced, environment), records(reader, kind, committed, environment)
    if ours == theirs:
        return True, f'{len(ours)} records'
    for index, (mine, its) in enumerate(zip(ours, theirs)):
        if mine != its:
            return False, (f'{len(ours)} produced vs {len(theirs)} committed records; '
                           f'row {index} differs (key {mine.split(" ")[0]} vs {its.split(" ")[0]})')
    return False, f'{len(ours)} produced vs {len(theirs)} committed records'


def compare(reader, produced, committed, environment=None):
    """Every file of both directories as `(name, status, detail)`; status is identical, pinned or differ."""
    produced, committed = Path(produced), Path(committed)
    names = sorted({path.name for path in produced.iterdir() if path.is_file()}
                   | {path.name for path in committed.iterdir() if path.is_file()})
    results = []
    for name in names:
        if name in PINNED:
            results.append((name, 'pinned', 'a pinned input, not produced by the recipe'))
        elif not (committed / name).is_file():
            results.append((name, 'differ', 'produced, but not in the committed tree'))
        elif not (produced / name).is_file():
            results.append((name, 'differ', 'committed, but the recipe does not produce it'))
        elif name in TREES or name in HASHES:
            same, detail = compare_dbm(reader, 'tree' if name in TREES else 'hash',
                                       produced / name, committed / name, environment)
            results.append((name, 'identical' if same else 'differ', detail))
        else:
            ours, theirs = (produced / name).read_bytes(), (committed / name).read_bytes()
            results.append((name, 'identical' if ours == theirs else 'differ',
                            f'{len(ours)} bytes' if ours == theirs else f'{len(ours)} produced vs {len(theirs)} committed bytes'))
    return results


def report(settings, results, out=sys.stdout):
    """Print one line per file and the cell's summary; return whether anything differs."""
    for name, status, detail in results:
        print(f'{settings.extension}/{name}: {status} ({detail})', file=out)
    counts = {status: sum(1 for _, got, _ in results if got == status) for status in ('identical', 'differ', 'pinned')}
    print(f'{settings.cell} ({settings.extension}): {counts["identical"]} identical, '
          f'{counts["differ"]} differ, {counts["pinned"]} pinned', file=out)
    return counts['differ'] > 0


def write_checksums(fixtures=FIXTURES):
    """Rewrite `fixtures.sha256` over every file of the tree, as `fixtures/w3/README.md` defines it."""
    fixtures = Path(fixtures)
    lines = []
    for path in sorted((p for p in fixtures.rglob('*') if p.is_file() and p.name != 'fixtures.sha256'),
                       key=lambda p: os.fsencode(p.relative_to(fixtures).as_posix())):
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        lines.append(f'{digest}  fixtures/w3/{path.relative_to(fixtures).as_posix()}\n')
    (fixtures / 'fixtures.sha256').write_text(''.join(lines), newline='\n')


# ── the model directory ──────────────────────────────────────────────

def model_dir(value, environment=None):
    """The verified extracted model directory, or a refusal naming the fetch."""
    environment = os.environ if environment is None else environment
    path = Path(value or environment.get('PINYIN_MODEL_DIR') or ROOT / 'target/model20/extracted').resolve()
    if not model.extraction_complete(path):
        raise Refused(f'fatal: {path} is not a complete model20 extraction '
                      f'({len(model.EXPECTED_MODEL_FILES)} files); run `just fetch-model` or pass --model-dir', 2)
    if not model.marker_matches(path.parent / 'verified'):
        raise Refused(f'fatal: {path.parent / "verified"} does not record the pinned archive digest; '
                      'the fixtures are deterministic against that archive only, so extract it with '
                      '`just fetch-model` (`--archive PATH` takes a local copy)', 2)
    return path


# ── the content .bin recipe (tools/generate_w3_fixtures.py, moved) ───

HEADER_FMT = '<7I'  # 28 bytes: 7 × u32 LE
HEADER_SIZE = 28
PRIMARY_IDX_SIZE = 35 * 4  # 140 bytes
ADDON_FILES = ('art.bin', 'culture.bin', 'economy.bin', 'geology.bin', 'history.bin', 'life.bin',
               'nature.bin', 'people.bin', 'science.bin', 'society.bin', 'sport.bin', 'technology.bin')


def sub_record_size(ng, fl):
    """Size of a simple sub-record (fl <= 2)."""
    if fl == 0:
        return 6 + ng * 8
    if fl == 1:
        return 14 if ng == 1 else 6 + 8 + (ng - 1) * 6 + 2
    if fl == 2:
        return 6 + (ng + 1) * 8
    return 0


def record_size(ng, fl, data, pos):
    """Compute size of a record at POS in DATA."""
    if fl <= 2:
        return sub_record_size(ng, fl)
    extra = fl - 1
    eff = ng + extra
    if ng < 10:
        base = 6 + eff * 8
        if ng >= 3:
            base += (ng - 2) * 2
        if fl == 4 and ng >= 5:
            base += 2
        return base
    inner = pos + 6
    bound = min(pos + 6 + eff * 30, len(data))
    while inner < bound:
        if inner + 2 > len(data):
            break
        s_ng, s_fl = data[inner], data[inner + 1]
        if s_ng == 0 or s_ng > 30 or s_fl not in (0, 1, 2):
            break
        size = sub_record_size(s_ng, s_fl)
        if size == 0 or inner + size > bound:
            break
        inner += size
    return max(inner - pos, 8)


def parse_records(data, data_start, nitems):
    """Return list of (offset, size) for each record."""
    found = []
    pos = data_start
    while pos < len(data) and len(found) < nitems:
        if pos + 2 > len(data):
            break
        ng, fl = data[pos], data[pos + 1]
        if ng == 0 or ng > 30:
            break
        size = record_size(ng, fl, data, pos)
        if pos + size > len(data):
            break
        found.append((pos, size))
        pos += size
    return found


def read_header(data):
    fields = struct.unpack_from(HEADER_FMT, data, 0)
    return dict(zip(('data_size', 'magic', 'nitems', 'version', 'capacity', 'data_size2', 'ngroups'), fields))


def build_header(hdr):
    """Build the 28-byte header, with data_size duplicated."""
    return struct.pack(HEADER_FMT, hdr['data_size'], hdr['magic'], hdr['nitems'], hdr['version'],
                       hdr['capacity'], hdr['data_size'], hdr['ngroups'])


def data_start_offset(nitems, ngroups=35):
    """Compute byte offset where the data section starts."""
    sec_entries = max(0, nitems - ngroups)
    preamble = 10 if sec_entries > 0 else 6
    return HEADER_SIZE + PRIMARY_IDX_SIZE + sec_entries * 4 + preamble


def build_indices(primary, secondary, nitems, ngroups=35):
    """Build the index region (primary + optional secondary + preamble)."""
    result = bytearray(primary)
    sec_entries = max(0, nitems - ngroups)
    if sec_entries > 0:
        result.extend(secondary[:sec_entries * 4])
        result.extend(b'\x00\x00\x64\x00\x00\x00\x64\x00\x00\x00')
    else:
        result.extend(b'\x00\x00\x00\x00\x00\x00')
    return bytes(result)


def truncate_content(data, max_records):
    """The first MAX_RECORDS records of one content table, as a rebuilt file; `(bytes, kept, original)`."""
    hdr = read_header(data)
    orig_items, ngroups = hdr['nitems'], hdr['ngroups']
    found = parse_records(data, data_start_offset(orig_items, ngroups), orig_items)
    keep = min(max_records, len(found))
    primary_idx = data[28:28 + PRIMARY_IDX_SIZE]
    sec_start = 28 + PRIMARY_IDX_SIZE
    secondary_idx = data[sec_start:sec_start + max(0, orig_items - ngroups) * 4]
    first, last = found[0], found[keep - 1]
    data_bytes = data[first[0]:last[0] + last[1]]
    new_hdr = dict(hdr, nitems=keep, capacity=max(hdr['capacity'], keep))
    header_bytes = bytearray(build_header(new_hdr))
    body = build_indices(primary_idx, secondary_idx, keep, ngroups) + data_bytes
    new_data_size = 28 + len(body) - 8
    struct.pack_into('<I', header_bytes, 0, new_data_size)
    struct.pack_into('<I', header_bytes, 20, new_data_size)
    return bytes(header_bytes) + body, keep, orig_items


def generate_content(src_dir, dst_dir, max_records, err=sys.stderr):
    """Truncate the oracle's content files into DST_DIR; return `{name: kept}`."""
    src_dir, dst_dir = Path(src_dir), Path(dst_dir)
    dst_dir.mkdir(parents=True, exist_ok=True)
    results = {}
    for name in ADDON_FILES:
        source = src_dir / name
        if not source.is_file():
            print(f'  SKIP {name} — not found', file=err)
            continue
        truncated, keep, orig_items = truncate_content(source.read_bytes(), max_records)
        (dst_dir / name).write_bytes(truncated)
        results[name] = keep
        print(f'  {name}: {keep}/{orig_items} records ({len(truncated)} bytes)', file=err)
    return results


def pin_ref_of(data_dir):
    """The `pin_ref=` line of the nearest `oracle-pin.txt` above DATA_DIR, or `unknown`."""
    directory = Path(data_dir).resolve()
    for _ in range(4):
        candidate = directory / 'oracle-pin.txt'
        if candidate.is_file():
            for line in candidate.read_text().splitlines():
                if 'pin_ref=' in line:
                    return line.strip()
            break
        directory = directory.parent
    return 'unknown'


def content(args, out=sys.stdout, err=sys.stderr):
    """The `content` subcommand: truncate the oracle's content tables."""
    dst_dir = Path(args.output_dir) if args.output_dir else FIXTURES
    if args.hash_only:
        for name in sorted(ADDON_FILES):
            path = dst_dir / name
            if path.is_file():
                print(f'{hashlib.sha256(path.read_bytes()).hexdigest()}  {name}', file=out)
        return 0
    if not Path(args.data_dir).is_dir():
        raise Refused(f'error: data directory not found: {args.data_dir}\n'
                      '  Specify --data-dir or install the pinned oracle')
    pin_ref = pin_ref_of(args.data_dir)
    print(f'Pin ref: {pin_ref}\nOutput:  {dst_dir}\n'
          f'Generating addon content fixtures (max {args.max_records} records each):', file=err)
    results = generate_content(args.data_dir, dst_dir, args.max_records, err)
    (dst_dir / 'pin-ref.txt').write_text(f'{pin_ref}\n', newline='\n')
    if dst_dir == FIXTURES:
        write_checksums(dst_dir)
    print(f'\nDone: {sum(results.values())} records across {len(results)} files', file=err)
    return 0


# ── the commands ─────────────────────────────────────────────────────

def regenerate(args, check, out=sys.stdout, err=sys.stderr):
    """Run the recipe for every selected cell; CHECK compares instead of writing."""
    model_path = model_dir(args.model_dir)
    cells = [name for name in args.cells.split(',') if name]
    for name in cells:
        if name not in CELLS:
            raise Refused(f'fatal: unknown cell {name!r}; cells are {", ".join(CELLS)}', 2)
        if sys.platform == 'win32' and name not in WINDOWS_CELLS:
            raise Refused(f'fatal: the {name} cell is unsupported on Windows: only the tkrzw cell has a Windows build; pass --cells tkrzw', 1)
    print(f'model: {model_path}', file=err)
    status = 0
    with tempfile.TemporaryDirectory(prefix='oxpinyin-w3-') as scratch:
        work = Path(scratch) / 'model'
        kept, kept_art = subset(model_path, work)
        print(f'subset: {len(kept)} system phrases, {len(kept_art)} art phrases', file=err)
        for name in cells:
            settings = settings_for(name)
            tools = build_tools(settings, err)
            produced = Path(scratch) / settings.extension
            produce(settings, tools, work, produced)
            committed = FIXTURES / settings.extension
            if not committed.is_dir():
                committed.mkdir(parents=True)
            results = compare(tools['raw_records'], produced, committed)
            if check:
                status |= int(report(settings, results, out))
                continue
            for file, verdict, _ in results:
                if verdict == 'differ' and (produced / file).is_file():
                    shutil.copyfile(produced / file, committed / file)
            report(settings, results, out)
            print(f'{settings.extension}: wrote every differing file; '
                  f'{sum(1 for _, v, _ in results if v == "identical")} left as committed', file=out)
    if not check:
        write_checksums()
        print('fixtures.sha256: rewritten', file=out)
    return status


def build_parser():
    parser = argparse.ArgumentParser(prog='just w3-fixtures', description=__doc__.split('\n\n')[0])
    sub = parser.add_subparsers(dest='command', required=True)
    for name, text in (('generate', 'regenerate fixtures/w3/<ext> from the model20 text'),
                       ('check', 'regenerate into a temporary directory and compare with the committed tree')):
        p = sub.add_parser(name, help=text)
        p.add_argument('--model-dir', help='the extracted model20 directory (default: $PINYIN_MODEL_DIR, then target/model20/extracted)')
        p.add_argument('--cells', default=','.join(CELLS), help='comma-separated cells (default: all)')
    p = sub.add_parser('content', help="truncate the oracle's content tables into the fixtures/w3 .bin files")
    p.add_argument('--data-dir', default=os.path.expanduser('~/.local/opt/pinyin-oracle/lib/libpinyin/data'),
                   help='oracle data directory')
    p.add_argument('--max-records', type=int, default=100, help='maximum records per fixture (default: 100)')
    p.add_argument('--output-dir', default=None, help='output directory (default: fixtures/w3/)')
    p.add_argument('--hash-only', action='store_true', help='print the SHA-256 of each content fixture and exit')
    return parser


def main(argv=None, out=sys.stdout, err=sys.stderr):
    """Parse the subcommand, run it and return its status."""
    try:
        args = build_parser().parse_args(argv)
    except SystemExit as error:
        return int(error.code or 0)
    try:
        if args.command == 'content':
            return content(args, out, err)
        return regenerate(args, args.command == 'check', out, err)
    except Refused as error:
        print(str(error), file=err)
        return error.status
