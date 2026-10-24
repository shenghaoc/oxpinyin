"""Per-cell delta ledgers for the kc and tkrzw cells, relative to the bdb ledger.

The pin's cells differ only in the DBM-conditional sources and config.h. A
delta ledger therefore measures only (a) every function defined in the cell's
DBM-conditional sources and (b) every shared function whose reachability from
the export roots differs from the bdb cell; every other row is inherited from
the bdb ledger by reference. Credit follows the bdb rule: compiler identities
by USR/signature, executed MATCH as credit, nonmatching execution discarded.
No capture, log or evidence file is written.
"""

from collections import Counter, defaultdict
import hashlib
import html
from pathlib import Path
import re
import subprocess
import sys

from compiler_cells import function_cells
from coverage_reviews import CELL_IDENTITY_REVIEWS, IDENTITY_REVIEWS, KNOWN_DEFECTS, MEASUREMENT_LIMITS, REVIEWS
from coverage_tiers import facility, FACILITIES
import gcov_measure as measure
from render_ledger import align, anchors, labels_text

TITLES = {'kc': 'Kyoto Cabinet', 'tkrzw': 'Tkrzw'}
ORIGINS = {'a': 'delta-a', 'b+': 'delta-b+', 'b-': 'delta-b-'}
PROVENANCE = r'Oracle shared-object SHA-256: libpinyin=`([0-9a-f]{64})`; libzhuyin=`([0-9a-f]{64})`'
# LLVM numbers a header function's link-local clones per translation unit
# (`operator==(_ChewingKey, _ChewingKey) (.4027)`); the number is link-time
# bookkeeping, not a source identity, so a clone family is matched across
# cells by its unsuffixed identity and its translation unit, one to one.
CLONE = re.compile(r' \(\.\d+\)$')
MUTATIONS = [
    ('J-m1', lambda n: n['file'] == 'src/pinyin.cpp' and n['name'] == 'pinyin_init'),
    ('J-m2', lambda n: n['file'] == 'src/pinyin.cpp' and n['line'] == 172 and n['name'] == 'check_format'),
]


def parse_rows(text):
    """Function rows of a ledger keyed by their compiler identity (unescaped)."""
    rows = {}
    for line in text.splitlines():
        if not line.startswith('| function/'):
            continue
        cells = [html.unescape(cell.strip()) for cell in line.strip('|').split('|')]
        if len(cells) != 9:
            raise RuntimeError('unexpected ledger row shape: ' + line[:80])
        location = re.match(r'\[(src/[^\]:]+):(\d+)\]', cells[1])
        if not location:
            raise RuntimeError('ledger row without a pin location: ' + cells[3])
        if cells[3] in rows:
            raise RuntimeError('duplicate ledger identity: ' + cells[3])
        rows[cells[3]] = dict(zip(('kind', 'source', 'signature', 'identity', 'export', 'path',
                                   'classification', 'credited', 'reason'), cells),
                              file=location.group(1), line=int(location.group(2)))
    return rows


def digest(keys):
    """Hash sorted newline-delimited identity strings, including a final newline."""
    return hashlib.sha256(('\n'.join(sorted(keys)) + '\n').encode()).hexdigest()


def base_identity(identity):
    """Remove one trailing LLVM link-local clone suffix from an identity."""
    return CLONE.sub('', identity)


def recorded_provenance(text):
    """Read the two recorded oracle digests, requiring one match and the expected pin.

    Raise RuntimeError when the ledger does not uniquely establish provenance.
    """
    matches = re.findall(PROVENANCE, text)
    if len(matches) != 1 or f'Pin `{measure.PIN}`' not in text:
        raise RuntimeError('the ledger does not record both oracle binary digests')
    return dict(zip(('libpinyin', 'libzhuyin'), matches[0], strict=True))


def prefix_provenance(args):
    """Oracle digests for a non-bdb cell.

    `--check` compares the prefix against the digests the existing ledger
    recorded. Generation records what it measured: the prefix manifest's
    libpinyin digest and the libzhuyin digest computed from the installed
    file, both verified again before any native comparison.
    """
    if args.check:
        return recorded_provenance(args.output.read_text())
    fields = dict(line.split('=', 1) for line in (args.oracle / 'oracle-pin.txt').read_text().splitlines()
                  if '=' in line and not line.startswith('#'))
    zhuyin = fields.get('zhuyin_shared_object_sha256') or hashlib.sha256((args.oracle / 'lib/libzhuyin.so').read_bytes()).hexdigest()
    provenance = {'libpinyin': fields['shared_object_sha256'], 'libzhuyin': zhuyin}
    if args.output.exists():
        previous = recorded_provenance(args.output.read_text())
        if previous != provenance:
            print(f'{args.cell}: oracle digests differ from the existing ledger; this regeneration records the current prefix', flush=True)
    return provenance


def conditional_files(pin, cell):
    """The cell's Makefile.am conditional translation units and private headers.

    The shared headers include `*_<suffix>.h` only under the cell's config.h
    macro (storage/ngram.h:29-40 and its siblings at the pin), and a header
    that only the cell's own files include (storage/bdb_utils.h and its
    kyotodb/tkrzwdb twins) exists only in this cell's inventory. A header any
    shared file includes stays shared.
    """
    shared = set.intersection(*(set(measure.cell_sources(pin, name)) for name in measure.CELLS))
    files = {'src/' + tu for tu in measure.cell_sources(pin, cell) if tu not in shared}
    suffix = measure.CELLS[cell]['suffix']
    files |= {str(path.relative_to(pin)) for path in (pin / 'src').rglob(f'*_{suffix}.h')}
    sources = {str(path.relative_to(pin)): path.read_text() for path in (pin / 'src').rglob('*') if path.suffix in {'.h', '.cpp'}}
    includers = defaultdict(set)
    for file, text in sources.items():
        for name in re.findall(r'^\s*#\s*include\s+"([^"]+)"', text, re.M):
            includers[Path(name).name].add(file)
    changed = True
    while changed:
        changed = False
        for file in sources:
            if file in files or not file.endswith('.h'):
                continue
            users = includers.get(Path(file).name, set())
            if users and users <= files:
                files.add(file)
                changed = True
    return files


def identity(nodes, key):
    """Use a node's nonempty USR as its identity, falling back to the graph key."""
    return nodes[key].get('usr') or key


def definitions(nodes):
    """List defined pin-node keys, treating an omitted pin flag as true."""
    return [key for key, node in nodes.items() if node.get('pin', True) and node.get('definition')]


def node_units(node):
    """The translation units LLVM bound a compiled body to (DICompileUnit basenames)."""
    return frozenset(unit for _, unit in node.get('llvm_bindings', set()) if unit)


def witness_unit(row):
    """The translation unit of a bdb ledger row in a link-local clone family.

    A clone is an internal-linkage copy of a header body and is callable only
    from its own translation unit, so the caller on the LLVM edge that ends
    its witness path is in that unit; a caller defined in a .cpp file names
    it. Any other shape fails the run instead of guessing.
    """
    steps = path_steps(row['path'])
    if len(steps) < 2 or not steps[-1][3].startswith('LLVM-') or not steps[-2][1].endswith('.cpp'):
        raise RuntimeError('translation unit of bdb clone row not derivable from its witness path: ' + row.get('identity', row['path'][-120:]))
    return Path(steps[-2][1]).name


def clone_families(state, bdb, bdb_files):
    """Base identities matched by translation unit: any carrying an LLVM
    link-local suffix, or occurring more than once among the bdb rows of
    shared sources or among the cell's definitions."""
    nodes = state['nodes']
    counts, suffixed = Counter(), set()
    for side, idents in (('bdb', [ident for ident, row in bdb.items() if row['file'] not in bdb_files]),
                         ('cell', [identity(nodes, key) for key in definitions(nodes)])):
        for ident in idents:
            counts[side, base_identity(ident)] += 1
            if CLONE.search(ident):
                suffixed.add(base_identity(ident))
    return frozenset(suffixed | {base for (_, base), count in counts.items() if count > 1})


def match_bdb(state, bdb, bdb_files, cell_keys, families):
    """One-to-one correspondence of bdb rows of shared sources to cell nodes.

    Identities match exactly. A clone family matches by base identity and
    translation unit, each row and node at most once, so several bdb clones
    can never be inherited through one cell clone. Returns {bdb identity:
    cell key} and the unmatched cell keys.
    """
    nodes = state['nodes']
    by_identity = {identity(nodes, key): key for key in cell_keys}
    family_cells, family_rows, matched = defaultdict(list), defaultdict(list), {}
    for key in cell_keys:
        base = base_identity(identity(nodes, key))
        if base in families:
            family_cells[base, node_units(nodes[key])].append(key)
    for ident, row in bdb.items():
        if row['file'] in bdb_files:
            continue
        base = base_identity(ident)
        if base in families:
            family_rows[base, frozenset({witness_unit(row)})].append(ident)
        elif ident in by_identity:
            matched[ident] = by_identity[ident]
    for unit_key, idents in family_rows.items():
        keys = family_cells.get(unit_key, [])
        if len(idents) > 1 or len(keys) > 1:
            raise RuntimeError(f'clone family {unit_key[0]} is ambiguous in translation unit {sorted(unit_key[1])}: '
                               f'bdb {sorted(idents)}, cell {sorted(identity(nodes, key) for key in keys)}')
        if keys:
            matched[idents[0]] = keys[0]
    return matched, set(cell_keys) - set(matched.values())


def partition(state, bdb, cell_files, bdb_files, reachable, executed):
    """Delta row keys: (a) conditional-source definitions with an export witness
    or an executed record, (b+) reachable here but matched by no bdb row,
    (b-) bdb rows of shared sources matched by no reachable cell node."""
    nodes = state['nodes']
    families = clone_families(state, bdb, bdb_files)
    rows = {key: 'a' for key in definitions(nodes)
            if nodes[key]['file'] in cell_files and (key in reachable or key in executed)}
    matched, unmatched = match_bdb(state, bdb, bdb_files, [key for key in definitions(nodes) if key in reachable], families)
    rows.update({key: 'b+' for key in unmatched if nodes[key]['file'] not in cell_files})
    missing = {ident: row for ident, row in bdb.items() if row['file'] not in bdb_files and ident not in matched}
    return rows, missing


def carrier(state, bdb, bdb_files, ident):
    """The cell node carrying a bdb identity, reachable or not: the same
    identity, or for a clone family the same base identity and translation unit."""
    families = clone_families(state, bdb, bdb_files)
    matched, _ = match_bdb(state, {ident: bdb[ident]}, bdb_files, definitions(state['nodes']), families)
    return matched.get(ident)


def path_steps(path):
    """Parse every rendered witness step into signature, file, line, and edge kind.

    Raise RuntimeError on a step that does not match the ledger path syntax.
    """
    steps = []
    for step in path.split(' → '):
        match = re.fullmatch(r'(.+) @ (\S+):(\d+) \(([\w/-]+)\)', step)
        if not match:
            raise RuntimeError('unparsable witness path step: ' + step[:120])
        steps.append((match.group(1), match.group(2), int(match.group(3)), match.group(4)))
    return steps


def break_point(state, reachable, row, families=frozenset()):
    """Where the bdb witness path stops being reachable in this cell."""
    nodes = state['nodes']
    located = defaultdict(set)
    for key, node in nodes.items():
        located[node['file'], node['line'], base_identity(node['signature'])].add(key)
    steps = path_steps(row['path'])
    for index, (signature, file, line, kind) in enumerate(steps, 1):
        keys = located.get((file, line, base_identity(signature)), set())
        if index == len(steps) and base_identity(row.get('identity', '')) in families:
            unit = witness_unit(row)
            keys = {key for key in keys if node_units(nodes[key]) == {unit}}
            if not keys:
                return f'step {index} `{signature}` @ {file}:{line} has no clone in translation unit {unit} in this cell'
        if not keys:
            return f'step {index} `{signature}` @ {file}:{line} is not defined in this cell\'s configured translation units'
        if not keys & set(reachable):
            return f'step {index} `{signature}` @ {file}:{line} has no export witness in this cell'
    return 'every bdb witness step is reachable here, but this identity is not: the call edge itself is absent in this cell'


def reviewed(ledger, cell, key, node):
    """Return a reviewed classification and cited mapping, or None without a rule.

    Prefer explicit mappings, then cell identities, shared identities, and
    source sites. Export-absence statuses receive T2-UNREACHABLE.
    """
    if key in ledger.MAPPINGS:
        return 'T5-REVIEWED', ledger.counterpart(key)
    review = CELL_IDENTITY_REVIEWS[cell].get(key) or IDENTITY_REVIEWS.get(key) or REVIEWS.get((node['file'], node['line']))
    if review is None:
        return None
    status, targets, reason = review
    category = 'T2-UNREACHABLE' if status.startswith('ABSENT_') and status.endswith('_EXPORTS') else 'T5-REVIEWED'
    return category, status + ' — ' + anchors(ledger.REPO, targets) + '; ' + reason


def emitted_inventory(out, pin):
    """Every instrumented function of both measurement objects, with no credit.

    Loading both libraries and exiting makes libgcov write a counter file for
    every linked object; only the identity set is read from those counters,
    and the counters are cleared again before any credited stage.
    """
    measure.clear(out)
    script = 'import ctypes, sys; [ctypes.CDLL(path) for path in sys.argv[1:]]'
    subprocess.run([sys.executable, '-c', script, str(out / 'libpinyin.so'), str(out / 'libzhuyin.so')], check=True, capture_output=True)
    records = set(measure.counters(out, pin))
    measure.clear(out)
    return records


def measure_cell(args, ledger, state, provenance):
    """Run cell measurements and align credited, failed, and limited compiler identities.

    Resolve required paths on args, require committed Rust sources, build
    instrumented libraries, and run contract, surface, user-store, and Rust
    gates. Remove registered measurement-limit credit and return attribution
    plus provenance/configuration and verdict records.
    """
    cell = args.cell
    nodes, roots = state['nodes'], state['roots']
    required = ['measurement_build', 'measurement_config', 'pinyin_so', 'zhuyin_so', 'rust_target', 'export', 'model']
    if any(getattr(args, name) is None for name in required):
        raise RuntimeError('delta ledger requires --measurement-build/config, --pinyin-so, --zhuyin-so, --rust-target, --export and --model')
    for name in required:
        setattr(args, name, getattr(args, name).resolve())
    surface_pinyin = (args.surface_pinyin_so or args.pinyin_so).resolve()
    if ledger.run(['git', '-C', ledger.REPO, 'status', '--porcelain', '--', 'crates']).strip():
        raise RuntimeError('Rust sources must match the committed crates tree')
    configuration = measure.build(args.pin, args.measurement_config, args.measurement_build, args.oracle, cell, provenance)
    libraries = measure.configured_libraries(args.measurement_config.parent / 'config.status', cell)
    behavior, nonmatching, incomplete, verdicts = measure.contracts(
        args.oracle, args.pinyin_so, args.zhuyin_so, args.measurement_build, args.pin, cell=cell, provenance=provenance)
    # Registered measurement limits: never credited, always a limit, whatever
    # the heap layout did to this run's measurement. A pin/Rust DIFFER stays.
    limits = {'contract/' + name for name in MEASUREMENT_LIMITS}
    verdicts = [(name, 'MEASUREMENT-DIFFER' if 'contract/' + name in limits and verdict in ('MATCH', 'MEASUREMENT-DIFFER') else verdict)
                for name, verdict in verdicts]
    incomplete.update('contract/' + name for name, verdict in verdicts if 'contract/' + name in limits and verdict != 'SKIP')
    for record in list(behavior):
        behavior[record] -= limits
        if not behavior[record]:
            del behavior[record]
    records = {'contract': verdicts}
    gate_result = measure.gates(args.rust_target, args.export, args.model)
    stages = {
        'same-data': measure.same_data(args.oracle, args.pinyin_so, args.measurement_build, args.pin, cell, provenance),
        'user-store': measure.user_store(args.oracle, args.rust_target, args.measurement_build, args.pin, cell, provenance, libraries),
        'surface': measure.surfaces(args.oracle, surface_pinyin, args.measurement_build, args.pin, cell, provenance,
                                    excused=measure.declared_residuals()),
    }
    for name, result in stages.items():
        executions, failed, limited, stage_verdicts = result
        for record, labels in executions.items():
            behavior[record].update(labels)
        nonmatching.update(failed)
        incomplete.update(limited)
        records[name] = stage_verdicts
    emitted = emitted_inventory(args.measurement_build, args.pin)
    compiled, _ = align(nodes, {record: {'gcc-emitted'} for record in emitted}, ledger.symbol_base)
    limited_roots, limited_cases, corrected, recovered = measure.limit_roots(
        args.oracle, args.measurement_build, incomplete, records['contract'], args.pin, cell)
    records['contract'] = [(name, verdict if 'contract/' + name in limits else corrected.get(name, verdict))
                           for name, verdict in records['contract']]
    for record, labels in recovered.items():
        if labels - limits:
            behavior[record].update(labels - limits)
    executed, unmatched = align(nodes, behavior, ledger.symbol_base)
    unknown_paths = ledger.witness(nodes, state['edges'], {export: roots[export] for export in limited_roots if export in roots})
    failed_nodes, _ = align(nodes, {record: {'nonmatching-execution'} for record in nonmatching}, ledger.symbol_base)
    return dict(configuration=configuration, executed=executed, unmatched=unmatched, compiled=compiled,
                failed_nodes=failed_nodes, unknown_paths=unknown_paths, records=records, incomplete=incomplete,
                limited_roots=limited_roots, limited_cases=limited_cases, gate_result=gate_result)


def evidence(key, node, executed):
    """The executed MATCH labels a row shows; a known defect's narrowed cases are left out."""
    labels = set(executed.get(key, set()))
    defect = KNOWN_DEFECTS.get((node['file'], node['line']))
    if defect:
        labels = {label for label in labels if not label.startswith(defect['excluded'])}
    return labels


def classify(ledger, cell, key, node, measured):
    """Choose a row's tier and explanation from defects, execution, reviews, and facilities.

    Known defects override execution credit; unresolved rows remain open
    findings with any counter or reachability limits described.
    """
    defect = KNOWN_DEFECTS.get((node['file'], node['line']))
    if defect:
        excluded = sum(label.startswith(defect['excluded']) for label in measured['executed'].get(key, set()))
        return 'KNOWN-DEFECT', (f"Known, tracked parity defect {defect['issue']}: {defect['reason']} "
                                f"{excluded} narrowed matching case(s) with the prefixes {', '.join(defect['excluded'])} are excluded from the evidence shown; no parity credit while {defect['issue']} is open.")
    if key in measured['executed']:
        mapping = 'Executed in matching differential(s); this is behavioral evidence for the exercised inputs, not a Rust internal-function identity or proof for all inputs.'
        review = reviewed(ledger, cell, key, node)
        if review:
            mapping += ' Source-reviewed counterpart/absence: ' + review[1]
        return 'BEHAVIOR-COVERED', mapping
    review = reviewed(ledger, cell, key, node)
    if review:
        return review
    name = facility(node)
    if name:
        return 'T3-FACILITY', f'Replaced by facility family `{name}` as defined in the bdb ledger; the cited Rust implementation and exact semantic scope are defined once there.'
    mapping = 'No executed MATCH evidence and no cited absence: this row is an open finding, not an established missing Rust implementation.'
    if key in measured['unknown_paths']:
        mapping += ' A fatal/discrepant case invoked an export that can reach this function; missing counters are unknown, not evidence of non-execution. Limit witness export: ' + measured['unknown_paths'][key][0] + '.'
    if key not in measured['compiled']:
        mapping += ' No exact GCC emitted-function alignment; the compiler identity needs resolution before any non-execution claim.'
    if key in measured['failed_nodes']:
        mapping += ' Execution was observed in a nonmatching case or discarded full replay; it is uncredited, not unexecuted.'
    return 'OPEN-FINDING', mapping


def split_verdicts(records):
    """Parity findings apart from measurement limits.

    DIFFER means the installed oracle and the Rust subject disagree.
    MEASUREMENT-DIFFER and TIMEOUT mean the original differential matched or
    did not finish but the instrumented pin build did not reproduce the
    oracle, so the case earns no credit; that is a limit on the evidence,
    not a parity finding. Surface groups fail the run on their own.
    """
    pairs = [(f'{stage}/{name}', verdict) for stage, results in records.items() if stage != 'surface'
             for name, verdict in results]
    return (sorted(label for label, verdict in pairs if verdict == 'DIFFER'),
            sorted(f'{label} ({verdict})' for label, verdict in pairs if verdict not in ('MATCH', 'SKIP', 'DIFFER')))


def toolchain():
    """Collect compiler and gcov version descriptions from subprocess stdout.

    Use the first version line, falling back to the first line or command name.
    """
    versions = []
    for argv in (['clang++', '--version'], ['opt', '--version'], ['g++', '--version'], ['gcov', '--version']):
        text = subprocess.run(argv, capture_output=True, text=True).stdout.strip().splitlines()
        versions.append(next((line.strip() for line in text if 'version' in line.lower()), text[0] if text else argv[0]))
    return '; '.join(versions)


def render_delta(args, state, provenance):
    """Measure a peer cell and write or compare its delta from the authorized bdb ledger.

    Validate provenance and one-to-one identity matching before measurements;
    re-read review rules, classify rows, and test deletion/restoration in
    memory. Print results and hashes. Raise on inconsistent output or, with
    require_complete, remaining open rows, defects, or non-MATCH results.
    """
    import importlib
    ledger = importlib.import_module('internal-ledger')
    cell = args.cell
    nodes, reachable, roots = state['nodes'], state['reachable'], state['roots']
    bdb_text = args.delta_from.read_text()
    bdb = parse_rows(bdb_text)
    if f'Pin `{ledger.PIN}`' not in bdb_text or 'Berkeley DB internal coverage ledger' not in bdb_text.splitlines()[0]:
        raise RuntimeError('--delta-from must name the authorized bdb ledger for this pin')
    bdb_provenance = recorded_provenance(bdb_text)
    cell_files = conditional_files(args.pin, cell)
    bdb_files = conditional_files(args.pin, 'bdb')
    families = clone_families(state, bdb, bdb_files)
    # Pre-flight before hours of measurement: clone-family matching, carriers
    # and break points must resolve (an ambiguous or underivable translation
    # unit fails here), and the residual declaration must parse.
    _, preflight = partition(state, bdb, cell_files, bdb_files, reachable, {})
    for ident in preflight:
        carrier(state, bdb, bdb_files, ident)
        break_point(state, reachable, preflight[ident], families)
    measure.declared_residuals()
    measured = measure_cell(args, ledger, state, provenance)
    executed = measured['executed']
    verified_data = measure.verify_prefix(args.oracle, provenance, cell)
    import coverage_reviews
    coverage_reviews.reload_rules()  # review batches written during the measurement apply

    def table(reachable_set):
        """Delta rows as rendered strings, keyed for the exact deletion checks."""
        keys, absent = partition(state, bdb, cell_files, bdb_files, reachable_set, executed)
        rendered, categories = {}, Counter()
        for key in sorted(keys, key=lambda k: (nodes[k]['file'], nodes[k]['line'], nodes[k]['signature'])):
            node = nodes[key]
            if key in reachable_set:
                cells = function_cells(ledger, state, key)
            else:
                cells = ['function/no-static-witness', ledger.pin_link(node), node['signature'], node.get('usr') or key, 'UNPROVEN',
                         'No static witness from an export root; executed under dispatch the graph does not resolve (counters aligned by exact identity)']
            category, mapping = classify(ledger, cell, key, node, measured)
            categories[category] += 1
            cells += [category, labels_text(evidence(key, node, executed)) or 'No credited execution; fatal/indirect analysis limits apply',
                      ORIGINS[keys[key]] + '; ' + mapping]
            rendered[key] = '| ' + ' | '.join(ledger.escape(c) for c in cells) + ' |'
        for ident in sorted(absent, key=lambda i: (absent[i]['file'], absent[i]['line'], absent[i]['signature'])):
            row = absent[ident]
            reason = break_point(state, reachable_set, row, families)
            key = carrier(state, bdb, bdb_files, ident)
            witness = 'bdb witness: ' + row['path'] + '; ' + reason
            if key is None:
                cells = ['function/not-defined-in-cell', row['source'], row['signature'], ident, 'UNPROVEN', witness,
                         'ABSENT-BY-RECIPE', 'No measurement: no object code in this cell',
                         ORIGINS['b-'] + '; the bdb row\'s definition is not compiled in this cell\'s configured translation units; bdb classification ' + row['classification'] + ' is not inherited.']
                categories['ABSENT-BY-RECIPE'] += 1
            else:
                node = nodes[key]
                category, mapping = classify(ledger, cell, key, node, measured)
                categories[category] += 1
                cells = ['function/bdb-witness-only', ledger.pin_link(node), node['signature'], ident, 'UNPROVEN', witness,
                         category, labels_text(evidence(key, node, executed)) or 'No credited execution; fatal/indirect analysis limits apply',
                         ORIGINS['b-'] + '; the bdb ledger credits this row as ' + row['classification'] + '; that credit is not inherited: ' + mapping]
            rendered['missing:' + ident] = '| ' + ' | '.join(ledger.escape(c) for c in cells) + ' |'
        return rendered, categories

    rows, categories = table(reachable)
    pristine = '\n'.join(rows[key] for key in rows).encode()
    checks = []
    for name, predicate in MUTATIONS:
        keys = [key for key, node in reachable.items() if predicate(node)]
        if len(keys) != 1:
            raise RuntimeError(f'{name}: exact preregistered identity not unique: {keys}')
        mutant_reachable = dict(reachable)
        del mutant_reachable[keys[0]]
        mutant, _ = table(mutant_reachable)
        passed, lost, extra = ledger.check_rows(rows, mutant)
        if passed or lost or extra != {'missing:' + identity(nodes, keys[0])}:
            raise RuntimeError(f'{name}: inventory deletion escaped detection')
        restored, _ = table(reachable)
        if not ledger.check_rows(rows, restored)[0] or '\n'.join(restored[key] for key in restored).encode() != pristine:
            raise RuntimeError(f'{name}: restoration changed ledger bytes')
        checks.append(f'{name}: deleting `{nodes[keys[0]]["name"]}` from the cell inventory produced exactly one new `delta-b-` row and was rejected; restored inventory accepted, byte-identical')
    first = next(key for key in rows if not key.startswith('missing:'))
    control = dict(rows)
    del control[first]
    passed, lost, extra = ledger.check_rows(rows, control)
    if passed or lost != {first} or extra:
        raise RuntimeError('delta row deletion escaped detection')
    checks.append(f'delta-row control: deleting the first table row (`{nodes[first]["signature"]}`) rejected; pristine row set accepted')

    rows_by_origin, missing = partition(state, bdb, cell_files, bdb_files, reachable, executed)
    pairs, _ = match_bdb(state, bdb, bdb_files, [key for key in definitions(nodes) if key in reachable], families)
    inherited = sorted(ident for ident, row in bdb.items() if row['file'] not in bdb_files and ident not in missing)
    if set(inherited) != set(pairs):
        raise RuntimeError('inherited rows and one-to-one matches disagree')
    recipe_absent = sorted(ident for ident, row in bdb.items() if row['file'] in bdb_files)
    clone_matched = sorted(ident for ident, key in pairs.items() if identity(nodes, key) != ident)
    origins = Counter(rows_by_origin.values())
    outside = [(key, node) for key, node in nodes.items() if node.get('pin', True) and node.get('definition')
               and node['file'] in cell_files and key not in reachable and key not in executed]
    with_code = sorted((node['file'], node['line'], node['signature']) for key, node in outside if node.get('llvm_bindings'))
    without_code = sorted((node['file'], node['line'], node['signature']) for key, node in outside if not node.get('llvm_bindings'))
    differ, limited = split_verdicts(measured['records'])
    open_rows = sorted(key for key, line in rows.items() if '| OPEN-FINDING |' in line)
    defect_rows = sorted(key for key, line in rows.items() if '| KNOWN-DEFECT |' in line)
    defect_issues = sorted({KNOWN_DEFECTS[(nodes[key]['file'], nodes[key]['line'])]['issue'] for key in defect_rows if not key.startswith('missing:')})
    contracts = Counter(verdict for _, verdict in measured['records']['contract'])
    static_tree = ledger.run(['git', '-C', ledger.REPO, 'rev-parse', 'HEAD:crates']).strip()
    hashes = measure.oracle_hashes(args.oracle, provenance, cell)
    subjects = {name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in
                (('libpinyin_capi', args.pinyin_so), ('libzhuyin_capi', args.zhuyin_so),
                 ('surface libpinyin_capi', (args.surface_pinyin_so or args.pinyin_so).resolve()))}
    title = TITLES[cell]
    env = cell.upper()
    spec = measure.CELLS[cell]
    text = f'''# Axis J: {title} internal coverage delta ledger

Regenerate with the clean pin, unchanged read-only {cell} prefix/configuration,
lane-owned debug {cell} Rust build/test artifacts (two build jobs maximum) and
the authorized bdb ledger it inherits from:

```sh
uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal tools/bisection/internal-ledger.py --cell {cell} \\
  --delta-from docs/findings/axis-j-bdb-ledger.md --pin "$LIBPINYIN_PIN" \\
  --config "${env}_CONFIG_H" --oracle "${env}_ORACLE_PREFIX" \\
  --build "$J_{env}_IR" --measurement-build "$J_{env}_GCOV" \\
  --measurement-config "${env}_CONFIG_H" \\
  --pinyin-so "${env}_RUST_PINYIN_SO" --zhuyin-so "${env}_RUST_ZHUYIN_SO" \\
  --surface-pinyin-so "${env}_RUST_SURFACE_SO" \\
  --rust-target "$J_{env}_RUST_TARGET" --export "${env}_EXPORT_DIR" \\
  --model "$MODEL20_DIR" --output docs/findings/axis-j-{cell}-ledger.md
```

`--check` repeats extraction and measurement and compares the ledger bytes;
`--require-complete` additionally rejects an open row, a known-defect row or a
non-MATCH verdict.
Build Rust facades with `cargo build --locked -p oxpinyin-capi -p oxpinyin-zhuyin-capi
--no-default-features --features {spec["feature"]}`; build the two requested test
binaries in `$J_{env}_RUST_TARGET/j-tests` with `cargo test --locked --profile dev
--config profile.dev.opt-level=1 -p pinyin-oracle -p oxpinyin-runtime
--no-default-features --features {spec["feature"]} --test sentence_surface_parity
--test user_dir_round_trip --no-run`, setting `CARGO_TARGET_DIR="$J_{env}_RUST_TARGET/j-tests"`
for this test build, and the separate `${env}_RUST_SURFACE_SO` with the same facade
cargo command and `--profile dev --config profile.dev.opt-level=1` in that test
target. Requirements and intermediates are those of the bdb ledger, with the
{title} headers and library in place of Berkeley DB.

Method: the pin's three cells differ only in the DBM-conditional sources and
config.h. This ledger measures the way the bdb ledger does (executed MATCH as
credit after the original oracle/Rust differential and the `-g -O0 --coverage`
measurement observation both match the reference; nonmatching execution
discarded; identities by exact USR/full signature) but carries only
(a) every function defined in this cell's DBM-conditional sources (its
Makefile.am conditional translation units, its `*_{spec["suffix"]}.h` headers and the
headers only those files include) that has an export witness or an executed
record (origin `delta-a`), and (b) every shared function whose export
reachability differs from the bdb cell: reachable here but absent from the bdb
ledger (`delta-b+`), or a bdb row with no export witness here (`delta-b-`,
rendered with the bdb witness and the step at which it stops; its bdb credit
is not inherited and the row is credited by this cell's own measurement or a
cited absence). Every other bdb row is inherited by reference and is not
re-measured. bdb rows defined in Berkeley DB conditional sources are absent
here by the recipe, not by a missing name. Rows match across cells one to
one: by exact identity, or, for a family of LLVM link-local clones of one
header body, by the unsuffixed identity and the translation unit (the cell
node's DICompileUnit; for a bdb row, the .cpp of the caller on the LLVM edge
that ends its witness path, the only unit an internal-linkage clone is
callable from). Each cell's own rows keep their exact identities.

'''
    text += f"Oracle shared-object SHA-256: libpinyin=`{hashes['libpinyin']}`; libzhuyin=`{hashes['libzhuyin']}`. The prefix manifest records the pinyin digest; the zhuyin digest was computed from the installed file at generation. Both digests are verified before any native comparison.\n\n"
    text += 'Subject shared-object SHA-256: ' + '; '.join(f'{name}=`{digest}`' for name, digest in subjects.items()) + ': the debug facades and the opt-level-1 surface facade that were measured, built from the recorded crates tree; a regeneration against other binaries differs here.\n\n'
    text += f'Pin `{ledger.PIN}`; subject crates tree `{static_tree}`. Read-only oracle manifest, shared-object hashes and **{verified_data}** data-file hashes verified. Measurement flags replace only configured CXXFLAGS with `-g -O0 --coverage`; `{spec["configure"]}`, shared-only and libzhuyin configure switches are verified against config.status, which also supplies the configured DBM link input. Static extraction uses `-std=c++17 -O0 -g -fno-inline -fno-discard-value-names -DHAVE_CONFIG_H`. Toolchain: {toolchain()}.\n\n'
    text += 'Pinning: this ledger records the crates tree and subject digests it measured, so any later change under crates/ makes `--check` fail until it is regenerated on that tree (#793).\n\n'
    text += f'Inherits from the bdb ledger: **{len(bdb)}** function rows, identity-set SHA-256 `{digest(bdb)}`, oracle digests libpinyin=`{bdb_provenance["libpinyin"]}`; libzhuyin=`{bdb_provenance["libzhuyin"]}`. Inherited by reference **{len(inherited)}** (of which **{len(clone_matched)}** link-local clones matched one to one by unsuffixed identity and translation unit); absent by recipe (Berkeley DB conditional sources) **{len(recipe_absent)}**; bdb rows without an export witness here (`delta-b-`) **{len(missing)}**.\n\n'
    text += f'Export roots **{len(roots)}/{len(state["expected_exports"])}**; TUs **{len(state["tus"])}/23**; reachable graph candidates in this cell **{len(reachable)}**. Delta rows **{len(rows)}**: conditional-source **{origins["a"]}**, reachable only here **{origins["b+"]}**, bdb-witness-only **{len(missing)}**. Classification: ' + ', '.join(f'{name} **{count}**' for name, count in sorted(categories.items())) + '.\n\n'
    text += f'Conditional-source definitions outside the measured closure, as in the bdb ledger (never declared unreachable; static dispatch closure is partial): **{len(with_code)}** with object code but neither an export witness nor an executed record' + (': ' + '; '.join(f'`{signature}` at {file}:{line}' for file, line, signature in with_code) if with_code else '') + f'; **{len(without_code)}** without object code in the configured translation units (uninstantiated template or unused inline bodies)' + (': ' + '; '.join(f'`{signature}` at {file}:{line}' for file, line, signature in without_code) if without_code else '') + '.\n\n'
    text += f'Unresolved LLVM indirect groups **{len(state["sites"])}**, pinned virtual groups **{len(state["resolved_sites"])}**, singleton typed callback groups **{len(state["singleton_callbacks"])}**, ambiguous virtual groups **{len(state["ambiguous"])}**. GCC executed records without an exact ledger alignment **{len(measured["unmatched"])}**; limited measurement entries **{len(measured["incomplete"])}**; fatal/discrepant root-call replay identifies **{len(measured["limited_cases"])}** limited contract cases and **{len(measured["limited_roots"])}** actually invoked export roots.\n\n'
    text += f'Contract cases **{len(measured["records"]["contract"])}**: ' + ', '.join(f'{name} {count}' for name, count in sorted(contracts.items())) + ' (SKIP marks cases whose fixture is another cell\'s storage format). Seven same-data drivers: ' + ', '.join(name + ' ' + verdict for name, verdict in measured['records']['same-data']) + '. User-store: ' + ', '.join(name + ' ' + verdict for name, verdict in measured['records']['user-store']) + '. Surface/C-ABI corpus exact MATCH replay: ' + '; '.join(f'{name} {matched}/{total} inputs ({other} not credited' + (': ' + ', '.join(f'`{text}`' for text in uncredited) if uncredited else '') + ')' for name, total, matched, other, uncredited in measured['records']['surface']) + '. Every input left uncredited is one of the six shared-comparison residuals declared in docs/findings/sentence-surface.md §12, the Rust gate\'s own membership source; any other sentence, tie or corpus difference, pin/Rust or measurement, fails the run. ' + measured['gate_result'] + '.\n\n'
    text += 'Non-MATCH differentials (parity findings, reported and not fixed here): ' + ('; '.join(differ) if differ else 'none') + '.\n\n'
    text += 'Measurement limits (the pin/Rust differential did not differ, but the instrumented `-g -O0 --coverage` pin build did not reproduce the installed oracle or a side timed out, so the case earns no credit): ' + ('; '.join(limited) if limited else 'none') + '.\n\n'
    text += 'Registered measurement limits (from the known-limit rule batch; never credited and always listed above, whatever a run\'s measurement did): ' + ('; '.join(f'`{name}`: {reason}' for name, reason in sorted(MEASUREMENT_LIMITS.items())) if MEASUREMENT_LIMITS else 'none') + '.\n\n'
    text += 'Open rows (no executed MATCH and no cited absence): ' + ('; '.join(f'`{nodes[key]["signature"] if not key.startswith("missing:") else key[8:]}`' for key in open_rows) if open_rows else 'none') + '.\n\n'
    text += f'Known, tracked parity defects (`KNOWN-DEFECT`, from the known-defect rule batch): **{len(defect_rows)}** rows' + (', ' + ', '.join(defect_issues) + ': ' + '; '.join(f'`{nodes[key]["signature"]}` at {nodes[key]["file"]}:{nodes[key]["line"]}' for key in defect_rows) if defect_rows else '') + '. Such a row is never credited while its issue is open; its executed records are shown as reach evidence with the cases narrowed around the defect excluded.\n\n'
    text += 'Facility families and tier definitions are those of the bdb ledger: ' + ', '.join(f'`{name}`' for name in FACILITIES) + '.\n\n'
    text += 'Exact preregistered deletion sensitivity (J-m1 deletes only `pinyin_init`; J-m2 deletes only `src/pinyin.cpp:172:check_format`, retaining the distinct zhuyin function). An inherited row is not in this table, so each deletion removes the identity from the cell inventory the delta is computed from and must surface as exactly one new `delta-b-` row:\n\n' + '\n'.join('- ' + check + '.' for check in checks) + '\n\n'
    text += '| Kind | Pin source/call site | Full signature | USR or signature identity | Witness export | Witness path / resolution | Classification | Credited differential | Counterpart or cited reason / remaining work |\n| --- | --- | --- | --- | --- | --- | --- | --- | --- |\n'
    text += '\n'.join(rows.values()) + '\n'
    if args.check:
        if args.output.read_bytes() != text.encode():
            raise RuntimeError('ledger differs from pristine regeneration')
    else:
        args.output.write_text(text)
    if args.output.read_bytes() != text.encode():
        raise RuntimeError('post-restoration tracked ledger bytes differ from generated pristine table')
    print(f'cell={cell} roots={len(roots)} reachable={len(reachable)} delta_rows={len(rows)} a={origins["a"]} b_plus={origins["b+"]} b_minus={len(missing)} inherited={len(inherited)} recipe_absent={len(recipe_absent)} categories={dict(categories)} open={len(open_rows)} known_defects={len(defect_rows)} differ={differ} limited={limited} unmatched_executed={len(measured["unmatched"])} outside_with_code={len(with_code)} outside_without_code={len(without_code)}')
    for check in checks:
        print(check)
    print('pristine ledger sha256=' + hashlib.sha256(text.encode()).hexdigest())
    if args.require_complete and (open_rows or defect_rows or differ or limited):
        raise RuntimeError('coverage remains PARTIAL; open rows, known-defect rows, non-MATCH differentials or measurement limits remain')
