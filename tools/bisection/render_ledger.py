"""Render the requested single table from identities and in-memory measurements."""

from collections import Counter, defaultdict
import hashlib
import html
from pathlib import Path
import re

from coverage_tiers import cited_items, facility, FACILITIES, sample_rows
import gcov_measure as measure
from coverage_reviews import REVIEWS, IDENTITY_REVIEWS

BASELINE = 'e723bc291a5c014679d58f2dd1b1c4726b2b899b'
BASELINE_ID_SHA256 = '8fffb373c81cc0dc92dd135d1870f9e53e2c720e9bce7a83c7e33812a9570613'
AUDITED_SEED = 10063648693516068216
AUDITED_SAMPLE = {'c:@F@pinyin_reset', 'c:@N@pinyin@S@UserTableInfo@F@get_open_counter#'}


def align(nodes, measurements, symbol_base):
    """Exact mangled symbol plus source extent and TU, not a name match.

    LLVM's link-local suffix is removed only from the symbol; its DICompileUnit
    still distinguishes header statics. Constructor ABI variants share USRs.
    Unmatched GCC records remain a limit, never a non-execution proof.
    """
    index = defaultdict(set)
    for key, node in nodes.items():
        bindings = node.get('llvm_bindings', set())
        if not bindings and node.get('symbol'):
            bindings = {(symbol_base(node['symbol']), '')}
        for symbol, tu in bindings:
            index[symbol, node['file']].add((key, tu))
    result, unmatched = defaultdict(set), set()
    for identity, labels in measurements.items():
        symbol, file, lo, hi, tu_object = identity
        candidates = index.get((symbol_base(symbol), file), set())
        # GCC emits these two header statics with C-linkage-style operator
        # symbols (_Zeq/_Zne), unlike clang's full Itanium parameter mangling.
        # Hand-reviewed join: immutable definition extent + complete clang
        # signature + actual compile unit. Never a general basename fallback.
        operator_identity = {
            ('_Zeq', 'src/storage/chewing_key.h', 81, 91):
                ('operator==(_ChewingKey, _ChewingKey)', '_Zeq11_ChewingKeyS_'),
            ('_Zne', 'src/storage/chewing_key.h', 93, 95):
                ('operator!=(_ChewingKey, _ChewingKey)', '_Zne11_ChewingKeyS_'),
        }.get((symbol, file, lo, hi))
        if not candidates and operator_identity:
            operator_signature, clang_symbol = operator_identity
            candidates = {
                (key, unit) for key, node in nodes.items()
                if node['file'] == file and node['line'] == lo
                # These G_BEGIN_DECLS header bodies are absent from the
                # libclang body inventory. The immutable pin range was read
                # by hand; a conflicting discovered extent must still fail.
                and node.get('body_end') in {None, hi}
                and re.sub(r' \(\.\d+\)$', '', node['signature']) == operator_signature
                for binding, unit in node.get('llvm_bindings', set())
                if unit and binding == clang_symbol
            }
        matched_here = False
        for key, tu in candidates:
            if not lo <= nodes[key]['line'] <= hi:
                continue
            if tu and tu != Path(tu_object).name.removesuffix('.o').split('-')[-1]:
                # TU names are actual source basenames from DICompileUnit;
                # object prefixes encode the storage/lookup directory.
                if not tu_object.endswith(tu):
                    continue
            matched_here = True
            result[key].update(labels)
            template = nodes[key].get('template_usr')
            if template in nodes:
                result[template].update(labels)
        if not matched_here:
            unmatched.add(identity)
    return result, unmatched


def citation(file, line):
    return f'[{file}:{line}](../../{file}#L{line})'


def anchors(root, targets):
    rendered = []
    for file, anchor in targets:
        lines = (root / file).read_text().splitlines()
        matches = [i + 1 for i, line in enumerate(lines) if anchor in line]
        if len(matches) != 1:
            raise RuntimeError(f'Rust implementation anchor not unique: {file}: {anchor}')
        rendered.append(citation(file, matches[0]))
    return '; '.join(rendered)


def labels_text(labels):
    groups = defaultdict(list)
    for label in sorted(labels):
        if label.startswith('contract/'):
            groups['contract'].append(label)
        else:
            groups[label].append(label)
    return '; '.join(values[0] + (f' (+{len(values)-1} other matching contract cases)' if len(values) > 1 else '') for values in groups.values())


def baseline_rows(text):
    """Read hash-guarded baseline membership from the one authorized ledger.

    No rewritten PR commit needs to remain locally available. Current source
    reachability is extracted independently of baseline membership/counts.
    """
    rows = {}
    for line in text.splitlines():
        if not line.startswith('| function/'):
            continue
        cells = [html.unescape(cell.strip()) for cell in line.strip('|').split('|')]
        if len(cells) > 7 and not cells[-1].startswith('baseline-962;'):
            continue
        rows[cells[3]] = cells
    digest = hashlib.sha256(('\n'.join(sorted(rows)) + '\n').encode()).hexdigest()
    if len(rows) != 962 or digest != BASELINE_ID_SHA256:
        raise RuntimeError('baseline-962 identity set differs from the original compiler inventory')
    return rows


def annotate_comparison(text, previous, reference):
    """Every differing cell is explicit in the existing table; no report file."""
    from finish_ledger import table_rows
    def identity(row):
        return ('function', row[3]) if row[0].startswith('function/') else (row[0], row[1], row[3])
    old = {identity(row): row for row in table_rows(previous)}
    names = ['kind', 'pin source', 'signature', 'identity', 'export', 'path/resolution',
             'classification', 'credited differential', 'counterpart/reason']
    counts = Counter()
    rebuilt = []
    current_keys = set()
    for line in text.splitlines():
        parsed = table_rows(line)
        if not parsed:
            rebuilt.append(line); continue
        row = parsed[0]
        key = identity(row); current_keys.add(key)
        prior = old.get(key)
        changes = [i for i in range(9) if prior is None or row[i] != prior[i]]
        if changes:
            counts.update(names[i] for i in changes)
            detail = ('new row' if prior is None else '; '.join(
                names[i] + ' previously: ' + (prior[i] or '(empty)') for i in changes))
            row[8] += '; Review delta from ' + reference[:8] + ': ' + detail
            line = '| ' + ' | '.join(html.escape(cell, quote=False).replace('|', '&#124;').replace('\n', ' ') for cell in row) + ' |'
        rebuilt.append(line)
    removed = set(old) - current_keys
    if removed:
        raise RuntimeError('comparison removed rows need explicit review: ' + repr(sorted(removed)))
    print('Per-row comparison against ' + reference + ': ' + repr(dict(counts)) +
          '; every changed cell has its prior value in that row; removed=0', flush=True)
    return '\n'.join(rebuilt) + '\n'


def render(args, state, contract_results, gate_receipt=None):
    # Import after internal-ledger has initialized: no compiler work at import.
    import importlib
    ledger = importlib.import_module('internal-ledger')
    # A resumed long measurement process may have imported the builder before
    # later tooling edits; its completed attribution is plain in-memory data.
    importlib.reload(measure)
    # A live compiler/audit coordinator can span a justified citation-rule
    # repair. Re-read the reviewed rule before selecting any category.
    import coverage_tiers
    importlib.reload(coverage_tiers)
    cited_items = coverage_tiers.cited_items
    sample_rows = coverage_tiers.sample_rows
    from finish_ledger import manual_sample, table_rows, verify_implicit_objects, source_mapping, POPULATION_MARK, SAMPLE_MARK, MANUAL_SEED, MANUAL_CRATES_TREE
    manual_population, manual_selected = manual_sample(table_rows(args.output.read_text()))
    if ledger.run(['git', '-C', ledger.REPO, 'rev-parse', 'HEAD:crates']).strip() != MANUAL_CRATES_TREE:
        raise RuntimeError('manual audit Rust tree changed; re-review the sample/source mappings')
    preloaded = contract_results is not None
    nodes, reachable, paths, roots = [state[name] for name in ['nodes', 'reachable', 'paths', 'roots']]
    verified_data = measure.verify_prefix(args.oracle)
    required = ['measurement_build', 'measurement_config', 'pinyin_so', 'zhuyin_so', 'rust_target', 'export', 'model']
    if any(getattr(args, name) is None for name in required):
        raise RuntimeError('dynamic ledger requires --measurement-build/config, --pinyin-so, --zhuyin-so, --rust-target, --export and --model')
    for name in required:
        setattr(args, name, getattr(args, name).resolve())
    if args.surface_pinyin_so is None and preloaded:
        alternate = args.rust_target / 'j-tests/debug/libpinyin_capi.so'
        if alternate.exists(): args.surface_pinyin_so = alternate
    surface_pinyin = (args.surface_pinyin_so or args.pinyin_so).resolve()
    # Protect semantic/sample reviews against a silently changed Rust checkout.
    if ledger.run(['git', '-C', ledger.REPO, 'status', '--porcelain', '--', 'crates']).strip():
        raise RuntimeError('Rust sources must match the reviewed crates tree')
    if contract_results is None:
        configuration = measure.build(args.pin, args.measurement_config, args.measurement_build, args.oracle)
        contract_results = measure.contracts(args.oracle, args.pinyin_so, args.zhuyin_so, args.measurement_build, args.pin)
    else:
        configuration = measure.command([args.measurement_config.parent / 'config.status', '--config']).strip()
    verify_implicit_objects(nodes, args.measurement_build, ledger.symbol_base)
    interrupted = (gate_receipt or {}).get('interrupted_cases', [])
    if interrupted:
        if not preloaded: raise RuntimeError('interruption receipt requires the private live-process continuation')
        contract_results = measure.reconcile_interrupted(contract_results, interrupted, args.oracle,
                                                        args.pinyin_so, args.zhuyin_so,
                                                        args.measurement_build, args.pin)
    behavior, nonmatching, incomplete, contract_verdicts = contract_results
    records = {'contract': contract_verdicts}
    gate_result = measure.preserved_gate(gate_receipt, args.rust_target) if gate_receipt is not None else measure.gates(args.rust_target, args.export, args.model)
    address = args.measurement_build.parent / 'j-dynamic.sock'
    if preloaded and not address.is_socket():
        partial, pending_ids = align(nodes, behavior, ledger.symbol_base)
        membership = baseline_rows((ledger.REPO / 'docs/findings/axis-j-bdb-ledger.md').read_text())
        print(f'contract-only alignment: baseline_executed_MATCH={sum(key in partial for key in membership)}/962; unique_current={sum(key in partial for key in reachable)}; unmatched_GCC_records={len(pending_ids)}', flush=True)
        input('PARALLEL_STAGES_PENDING (measurements remain in memory)\n')
        if not address.is_socket(): raise RuntimeError('parallel stage stream not ready')
    if preloaded and address.is_socket():
        stages, compiled_records = measure.consume_parallel(address, measure.identity_guard(args.oracle, args.pinyin_so, args.zhuyin_so, args.measurement_config, surface_pinyin))
    else:
        stages = {
            'same-data': measure.same_data(args.oracle, args.pinyin_so, args.measurement_build, args.pin),
            'user-store': measure.user_store(args.oracle, args.rust_target, args.measurement_build, args.pin),
            'surface': measure.surfaces(args.oracle, surface_pinyin, args.measurement_build, args.pin),
        }
        compiled_records = set(measure.counters(args.measurement_build, args.pin))
    for name, result in stages.items():
        executions, failed, limited, verdicts = result
        for identity, labels in executions.items():
            behavior[identity].update(labels)
        nonmatching.update(failed); incomplete.update(limited)
        records[name] = verdicts
    compiled, compiled_unmatched = align(nodes, {identity: {'gcc-emitted'} for identity in compiled_records}, ledger.symbol_base)
    limited_roots, limited_cases, corrected, recovered = measure.limit_roots(args.oracle, args.measurement_build, incomplete, records['contract'], args.pin)
    records['contract'] = [(name, corrected.get(name, verdict)) for name, verdict in records['contract']]
    for identity, labels in recovered.items(): behavior[identity].update(labels)
    executed, unmatched = align(nodes, behavior, ledger.symbol_base)
    unknown_paths = ledger.witness(nodes, state['edges'], {export: roots[export] for export in limited_roots if export in roots})
    failed_nodes, failed_unmatched = align(nodes, {identity: {'nonmatching-execution'} for identity in nonmatching}, ledger.symbol_base)
    # Preserve the original 962 candidates even when receiver refinement removes
    # a conservative path. Such omission does not prove the function unreachable.
    old = baseline_rows((ledger.REPO / 'docs/findings/axis-j-bdb-ledger.md').read_text())
    missing = set(old) - set(nodes)
    if missing:
        raise RuntimeError('baseline compiler identities disappeared: ' + repr(sorted(missing)[:10]))
    refined_count = len(reachable)
    retained = set(old) - set(reachable)
    inventory = dict(reachable)
    inventory.update({key: nodes[key] for key in retained})
    added = set(inventory) - set(old)
    cited = cited_items(ledger.REPO, args.pin, inventory, tightened='role')
    sample = set(sample_rows(cited, args.sample_seed))
    if args.sample_seed != AUDITED_SEED or len(cited) != 29 or sample != AUDITED_SAMPLE:
        raise RuntimeError('citation rule/sample drift: inspect both sides and explicitly revise the audited sample')
    categories, rows, gaps = Counter(), {}, defaultdict(list)
    family_references = {}
    for name, family in FACILITIES.items():
        family_references[name] = anchors(ledger.REPO, family['targets'])
    for key, node in sorted(inventory.items(), key=lambda pair: (pair[1]['file'], pair[1]['line'], pair[1]['signature'])):
        from compiler_cells import function_cells
        compiler_prefix = function_cells(ledger, state, key)
        export, trail = compiler_prefix[4:6]
        behavior_text = labels_text(executed.get(key, set()))
        if key in executed:
            category = 'BEHAVIOR-COVERED'
            mapping = 'Executed in matching differential(s); this is behavioral evidence for the exercised inputs, not a Rust internal-function identity or proof for all inputs.'
            reviewed = source_mapping(ledger, key, node)
            if reviewed:
                mapping += ' Source-reviewed counterpart/absence: ' + reviewed[1]
        elif key in cited:
            category = 'T1-CITED'
            item = cited[key]
            mapping = f"Rust item {citation(item['file'], item['line'])}; comment at {citation(item['file'], item['comment_line'])} literally cites `{item['citation']}` inside this clang body and names its function/port role."
            if key in sample:
                mapping += ' Random 5% sample: both implementations read; correspondence accepted.'
        elif facility(node):
            category = 'T3-FACILITY'
            name = facility(node)
            mapping = f'Replaced by facility family [{name}](#family-{name}); the cited Rust implementation and exact semantic scope are defined once above.'
        elif key in ledger.MAPPINGS:
            category = 'T5-REVIEWED'
            mapping = ledger.counterpart(key)
        elif key in IDENTITY_REVIEWS or (node['file'], node['line']) in REVIEWS:
            category = 'T5-REVIEWED'
            status, targets, reason = IDENTITY_REVIEWS.get(key, REVIEWS.get((node['file'], node['line'])))
            if status == 'ABSENT_BDB_EXPORTS': category = 'T2-UNREACHABLE'
            mapping = status + ' — ' + anchors(ledger.REPO, targets) + '; ' + reason
        else:
            category = 'UNKNOWN-LIMIT' if key in unknown_paths or key not in compiled or key in failed_nodes else 'NOT-EXECUTED-UNREVIEWED'
            mapping = 'No executed MATCH evidence or accepted static classification. Additional differential coverage/review needed; this is not an established missing Rust implementation.'
            if key in unknown_paths:
                mapping += ' Fatal/discrepant case invoked an export that can reach this function; missing counters are unknown, not evidence of non-execution. Limit witness export: ' + unknown_paths[key][0] + '.'
            if key not in compiled:
                mapping += ' No exact GCC emitted-function alignment; generic/body or compiler identity needs resolution before any non-execution claim.'
            if key in failed_nodes:
                mapping += ' Execution was observed in a nonmatching case or discarded full replay; it is uncredited, not unexecuted. A full replay footprint does not identify the individual input that executed it.'
            if category == 'NOT-EXECUTED-UNREVIEWED': gaps[node['file']].append((key, export, trail))
        categories[category] += 1
        if key in cited and category == 'BEHAVIOR-COVERED':
            item = cited[key]
            mapping += f" Also T1 citation: {citation(item['file'], item['line'])}, literal `{item['citation']}` at comment {item['comment_line']}."
        origin = 'baseline-962' if key in old else 'added-current-analysis'
        if key in manual_population: mapping += '; ' + POPULATION_MARK
        if key in manual_selected: mapping += '; ' + SAMPLE_MARK
        cells = compiler_prefix + [category, behavior_text or 'No credited execution; fatal/indirect analysis limits apply', origin + '; ' + mapping]
        rows[key] = '| ' + ' | '.join(ledger.escape(cell) for cell in cells) + ' |'
    mutations = ledger.mutation_check(rows, inventory)
    counts = ', '.join(f'{name} **{count}**' for name, count in sorted(categories.items()))
    contracts = Counter(verdict for _, verdict in records['contract'])
    diff_counts = ', '.join(f'{name} {count}' for name, count in sorted(contracts.items()))
    static_tree = ledger.run(['git', '-C', ledger.REPO, 'rev-parse', 'HEAD:crates']).strip()
    text = '''# Axis J: Berkeley DB internal coverage ledger

Regenerate with the clean pin, unchanged read-only bdb prefix/configuration,
and lane-owned debug bdb Rust build/test artifacts (two build jobs maximum):

```sh
uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal tools/bisection/internal-ledger.py --pin "$LIBPINYIN_PIN" \\
  --config "$BDB_CONFIG_H" --oracle "$BDB_ORACLE_PREFIX" \\
  --build "$J_BDB_IR" --measurement-build "$J_BDB_GCOV" \\
  --measurement-config "$BDB_CONFIG_H" \\
  --pinyin-so "$BDB_RUST_PINYIN_SO" --zhuyin-so "$BDB_RUST_ZHUYIN_SO" \\
  --surface-pinyin-so "$BDB_RUST_SURFACE_SO" \\
  --rust-target "$J_BDB_RUST_TARGET" --export "$BDB_EXPORT_DIR" \\
  --model "$MODEL20_DIR" --output docs/findings/axis-j-bdb-ledger.md
```

`--check` repeats extraction and measurement and compares the ledger bytes;
`--require-complete` additionally rejects an unexplained row or an unresolved
analysis limit. `--inventory-check` independently rebuilds every compiler-owned cell (kind,
source, full signature/USR, witness export/path and boundary/type-lead explanation), reruns the exact J-m1/J-m2 deletions/restoration and verifies the tracked
ledger stays byte-identical; it reuses no compiler row set and does not repeat
the unchanged behavioral suites. Build Rust facades with `cargo build --locked -p oxpinyin-capi
-p oxpinyin-zhuyin-capi --no-default-features --features bdb`; build the two
requested test binaries in `$J_BDB_RUST_TARGET/j-tests` with
`cargo test --locked --profile dev --config profile.dev.opt-level=1 -p pinyin-oracle
-p oxpinyin-runtime --no-default-features --features bdb --test
sentence_surface_parity --test user_dir_round_trip --no-run`, setting
`CARGO_TARGET_DIR="$J_BDB_RUST_TARGET/j-tests"` for this test build. Test executables use an optimized development
profile with debug assertions/overflow checks retained; the C-ABI comparison
libraries for contract/seven-driver/user-store comparisons use the original
unoptimized development profile. Build the separate `$BDB_RUST_SURFACE_SO`
with the same facade cargo command and `--profile dev --config
profile.dev.opt-level=1`, setting `CARGO_TARGET_DIR` to the test target.
Surface/corpus credit is specific to that development artifact, not a claim
about every optimization profile. Only IR/object, executable and gcov instrumentation
intermediates are written; AST, IR text, opt graph, observations, dumps and gcov
JSON remain in memory. No capture, log, report or evidence archive is written.
Requirements: Python 3, matching clang/libclang/LLVM tools, GCC/gcov with JSON
format 2, GLib and Berkeley DB headers, the supported Rust toolchain and inputs.

This revision follows the maintainer's requested method change from the static
preregistration: executed MATCH is stronger evidence for an exercised behavior
than a comment/name mapping. A separate `-g -O0 --coverage` GCC build uses the
unchanged bdb configure output and exact conditional source/link recipe, with
assertions live. It does **not** replace the installed oracle. A function is
credited only after the original oracle/Rust differential matches and the
measurement observation also matches the reference. Nonmatching execution is
discarded. Corpus/surface coverage is replayed after resetting counters with
only exact MATCH inputs; the six declared supplementary residuals are checked
by the existing Rust gate but are not reclassified as exact C-ABI MATCH.

Gcov functions align to actual mangled LLVM/libclang identities by source body
extent and translation unit. Full signatures/USRs, not spelling similarity,
identify overloads and instances. Successful `os._exit` workers use a private
measurement-only `__gcov_dump` shim, retaining the harness's teardown semantics.
The private SIGABRT handler dumps counters, restores default disposition and
re-raises; credit requires its completion marker and unchanged signal/reference
fields. Other fatal workers and failed forked sentence probes can lose counters. Their missing
counters do not establish non-execution. An executed function behind unresolved
dispatch has behavioral evidence for that input; gcov is not a call-edge trace,
does not guess the callee of every indirect site and proves nothing for all inputs.

'''
    hashes = measure.oracle_hashes(args.oracle)
    text += f"Oracle shared-object SHA-256: libpinyin=`{hashes['libpinyin']}`; libzhuyin=`{hashes['libzhuyin']}`. The legacy read-only prefix manifest records pinyin; the recorded zhuyin digest was independently cross-checked against the original configured pin build. Both digests are verified before any native comparison.\n\n"
    text += f'Pin `{ledger.PIN}`; subject crates tree `{static_tree}`. Read-only oracle manifest, shared-object hash and **{verified_data}** data-file hashes verified. Measurement flags replace only configured CXXFLAGS with `-g -O0 --coverage`; bdb, shared-only and libzhuyin configure switches are verified against config.status. Static extraction uses `-std=c++17 -O0 -g -fno-inline -fno-discard-value-names -DHAVE_CONFIG_H`. LLVM constructor/destructor ABI entry variants share source USRs; headers and the 23 configured translation units come from the immutable pin. Other-backend source TUs are absent by the recipe, not from a missing name.\n\n'
    text += f'Export roots **{len(roots)}/{len(state["expected_exports"])}**; TUs **{len(state["tus"])}/23**; refined graph candidates **{refined_count}**; previous conservative-only rows retained **{len(retained)}**; ledger function rows **{len(inventory)}**, non-export **{len(inventory)-len(roots)}**. Original compiler-identity baseline: **962** rows, of which executed MATCH **{sum(key in executed for key in old)}**. Current classification: {counts}. No missing Rust counterpart is asserted from an unreviewed row.\n\n'
    text += f'Additional candidates relative to baseline 962: **{len(added)}**, from explicit CXXDeleteExpr destructor edges and exact LLVM ABI alias resolution. They do not enlarge the baseline denominator. Actual export witnesses: ' + '; '.join(f'{ledger.pin_link(nodes[key])} `{nodes[key]["signature"]}` (export `{paths[key][0]}`)' for key in sorted(added)) + f'. Additional-candidate executed MATCH count **{sum(key in executed for key in added)}**. The table marks baseline/additional origin separately. Its baseline membership is guarded by the original identity-set SHA-256, while current graph extraction stays independent; baseline membership needs no historical commit. The optional per-row review comparison requires its named commit to be available in the checkout.\n\n'
    surface_limits = sorted(label for label in incomplete if label.startswith('surface/'))
    text += f'Unresolved LLVM indirect groups **{len(state["sites"])}**, pinned virtual groups **{len(state["resolved_sites"])}**, singleton typed callback groups **{len(state["singleton_callbacks"])}**, ambiguous virtual groups **{len(state["ambiguous"])}**. GCC executed records without an exact ledger alignment **{len(unmatched)}**; limited measurement entries (workers or surface groups) **{len(incomplete)}**. Forked surface limits: ' + (', '.join(surface_limits) or 'none') + f'. Known complete abort-flush framing is parsed separately from unchanged JSON observations; unknown/incomplete framing fails without credit. Individual child observations remain a limit even when the aggregate MATCH replay has valid counters. Fatal/discrepant root-call replay identifies **{len(limited_cases)}** limited contract cases and **{len(limited_roots)}** actually invoked export roots; their conservative descendants remain UNKNOWN where no other tier accounts for them. These limits prevent exhaustive static closure and can hide further targets; omitted candidates are never declared unreachable.\n\n'
    by_differential = Counter()
    for key in old:
        labels = executed.get(key, set())
        names = {'contract/exact-MATCH-cases' if label.startswith('contract/') else label for label in labels}
        by_differential.update(names)
    text += 'Unique executed MATCH functions within baseline 962, per differential (overlap retained): ' + '; '.join(f'{name} **{count}**' for name, count in sorted(by_differential.items())) + '.\n\n'
    text += f'Contract cases **{len(records["contract"])}**: {diff_counts}. Seven same-data drivers: ' + ', '.join(name + ' ' + verdict for name, verdict in records['same-data']) + '. User-store: ' + ', '.join(name + ' ' + verdict for name, verdict in records['user-store']) + '.\n\n'
    text += 'Surface/C-ABI corpus exact MATCH replay: ' + '; '.join(f'{name} {matched}/{total} inputs ({other} not credited)' for name, total, matched, other in records['surface']) + '. ' + gate_result + '.\n\n'
    text += f'T1 eligible citation rows **{len(cited)}**; random seed `{AUDITED_SEED}`, ceil(5%) **{len(sample)}** sampled identities: ' + ', '.join('`' + key + '`' for key in sorted(sample)) + '. Both bodies were read. Earlier broad-body citations failed semantic review (warning wrappers, locale-only initialization, partial predicates, configuration-field copying and nonshipping training helpers); the rule was tightened to contiguous actual shipping items, full cited ranges within clang bodies, and an explicit same-function/port role, then resampled. The accepted rule/sample is guarded against drift. T2 configured-out rows **0** within this bdb reachable inventory. T4 registered-divergence rows **0**: row 11 covers the six declared floating-point residuals, not whole lookup or container functions. Facility families classify only their stated storage/lifetime operations.\n\n'
    text += 'Rebase citation audit: the new C-API comments initially made 30 rows eligible and selected SubPhraseIndex::add_unigram_frequency. Reading its complete pin body and Rust helper rejected that citation as a whole-function counterpart: the helper combines facade accounting with a USER_FILE overflow/update scope and omits the pin index lookup/error branches. The exact reviewed source/USR exclusion is in coverage_tiers.py; it is not a divergence or an unreachability claim. Fresh executed MATCH credit remains independent of this citation limit. The tightened population is resampled with the same seed and independently guarded above.\n\n'
    for name, family in FACILITIES.items():
        text += f'<a id="family-{name}"></a>Facility `{name}`: {family_references[name]}. {family["reason"]}\n\n'
    text += f'Manual audit: **20/178** original manual identities, seed `{MANUAL_SEED}`, **20 accepted, 0 wrong** (14 counterparts and 6 absent/inlined wrappers). The marked rows identify the sample and preserved population; hash guards reject selection drift. Source review checked sorting/wildcard predicates, both keyspaces/prefix markers and indexed reading removal/mask state. No full-category re-review trigger.\n\n'
    text += '**Function-accounting reconciliation; static dispatch remains partial.** Unknown functions are distinct from demonstrably not-executed, unexplained functions and confirmed missing implementations. Fatal counters, measurement discrepancies, unmatched identities and unresolved dispatch are explicit limits; no gap issue is manufactured from lost fatal counters. The table keeps one export witness and its direct/pinned/conservative status for every static candidate. No production fix or new divergence class is made.\n\n'
    text += 'Exact preregistered deletion sensitivity (PR #522 §16 at `a628763a`, addendum 5):\n\n' + '\n'.join('- ' + result + '.' for result in mutations) + '\n\n'
    text += 'J-m1 deletes only `pinyin_init`; J-m2 deletes only `src/pinyin.cpp:172:check_format`, retaining the distinct zhuyin function. Each independent compiler row-set check rejects the exact mutation, restores the in-memory copy byte for byte, and accepts pristine. No mutant source is committed.\n\n'
    text += '| Kind | Pin source/call site | Full signature | USR or signature identity | Witness export | Witness path / resolution | Classification | Credited differential | Counterpart or cited reason / remaining work |\n| --- | --- | --- | --- | --- | --- | --- | --- | --- |\n'
    text += '\n'.join(rows.values()) + '\n'
    from compiler_cells import boundary_cells
    for cells in boundary_cells(ledger, state, executed):
        text += '| ' + ' | '.join(ledger.escape(cell) for cell in cells) + ' |\n'
    if args.compare_ref:
        if not re.fullmatch('[0-9a-f]{40}', args.compare_ref):
            raise RuntimeError('--compare-ref requires the full reviewed commit identity')
        previous = ledger.run(['git', '-C', ledger.REPO, 'show', args.compare_ref + ':docs/findings/axis-j-bdb-ledger.md'])
        header, table = text.split('| Kind |', 1)
        header = header.replace('--model "$MODEL20_DIR" --output docs/findings/axis-j-bdb-ledger.md',
                                '--model "$MODEL20_DIR" --output docs/findings/axis-j-bdb-ledger.md \\\n  --compare-ref ' + args.compare_ref)
        header += ('Per-row review comparison against `' + args.compare_ref + '`: the final cell of every changed row lists each changed field and its previous value. Current cells come from fresh compiler extraction and all fresh workloads; previous values provide review context only. Compiler fields are rebuilt; classification follows the measured/reviewed tier priority; workload labels merge all fresh exact MATCH attribution, replacing the old incremental J-refresh label group. Counterpart/reason changes follow the freshly selected tier and explicit source review. No prior behavioral footprint is reused. Import-time cell provenance for the old process was not recorded; this run sets and asserts `bdb` before import.\n\n')
        text = annotate_comparison(header + '| Kind |' + table, previous, args.compare_ref)
    if args.check:
        if args.output.read_bytes() != text.encode(): raise RuntimeError('ledger differs from pristine regeneration')
    else:
        args.output.write_text(text)
    if args.output.read_bytes() != text.encode():
        raise RuntimeError('post-restoration tracked ledger bytes differ from generated pristine table')
    print(f'roots={len(roots)} refined={refined_count} retained={len(retained)} inventory={len(inventory)} baseline_executed={sum(key in executed for key in old)} categories={dict(categories)} unresolved={len(state["sites"])} unmatched_executed={len(unmatched)}')
    print('gaps per pin source file: ' + repr({file: len(items) for file, items in gaps.items()}))
    for result in mutations: print(result)
    print('pristine ledger sha256=' + hashlib.sha256(text.encode()).hexdigest())
    if args.require_complete and (gaps or categories['UNKNOWN-LIMIT'] or state['sites'] or state['ambiguous'] or unmatched or incomplete):
        raise RuntimeError('coverage remains PARTIAL; explicit analysis limits/unreviewed functions remain')
