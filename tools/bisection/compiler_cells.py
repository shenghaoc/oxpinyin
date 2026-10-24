"""Fresh compiler-owned ledger cells; behavioral columns are deliberately excluded."""

from collections import Counter, defaultdict


def function_cells(ledger, state, key):
    """Build six compiler-owned table cells for one function identity.

    Prefer a direct witness, then a pinned-virtual witness, then a conservative
    path; retained baseline identities without a path are marked UNPROVEN.
    """
    nodes = state['nodes']
    node = nodes[key]
    for mode, preferred in [('direct', state['direct_paths']),
                            ('pinned-virtual', state['pinned_paths']),
                            ('conservative', state['paths'])]:
        if key in preferred:
            export, path = preferred[key]
            return ['function/' + mode, ledger.pin_link(node), node['signature'],
                    node.get('usr') or key, export, ledger.format_path(nodes, path)]
    # Membership retention never copies a stale witness or asserts deadness.
    return ['function/baseline-conservative', ledger.pin_link(node), node['signature'],
            node.get('usr') or key, 'UNPROVEN',
            'No current compiler witness; retained baseline identity requires review']


def boundary_cells(ledger, state, executed=None):
    """Build boundary rows for indirect, virtual, callback, and external calls.

    Optional executed labels annotate callers without resolving additional
    static targets.
    """
    from render_ledger import labels_text
    executed = executed or {}
    nodes, paths = state['nodes'], state['paths']
    result = []
    field_sites = defaultdict(set)
    for caller, line, owner, field, usr, typ, header in state['fields']:
        if caller in state['ast']:
            field_sites[state['ast'][caller]['file'], line].add(f'{owner}::{field}; declaration USR `{usr}`; type `{typ}`; header `{header}`')
    for key, line in state['sites']:
        node = dict(nodes[key], line=line or nodes[key]['line'])
        export, path = paths[key]
        status = 'caller executed MATCH; target unresolved' if key in executed else 'UNRESOLVED; no executed caller evidence'
        fields = '; '.join(sorted(field_sites[nodes[key]['file'], line]))
        cells = ['indirect', ledger.pin_link(node), nodes[key]['signature'], key, export, ledger.format_path(nodes, path), status, labels_text(executed.get(key, set())), 'Actual LLVM callee value unresolved; no static unreachability or target inference.' + (' AST typed field lead (not implementation proof): ' + fields if fields else '')]
        result.append(cells)
    for a, b, choices in state['ambiguous']:
        cells = ['virtual', ledger.pin_link(nodes[a]), state['ast'].get(b, {}).get('signature', b), b, paths[a][0], '; '.join(nodes[c]['signature'] for c in choices if c in nodes) or 'no known concrete override', 'UNRESOLVED', labels_text(executed.get(a, set())), 'Actual overridden-method hierarchy; receiver does not uniquely pin this target.']
        result.append(cells)
    for key, line, target in state['resolved_sites']:
        cells = ['pinned virtual', ledger.pin_link(dict(nodes[key], line=line)), nodes[target]['signature'], target, paths[key][0], 'Caller USR, line, static receiver and exact overridden-method chain give one target', 'PINNED', labels_text(executed.get(target, set())), ledger.pin_link(nodes[target])]
        result.append(cells)
    for key, line, target, prototype in state['singleton_callbacks']:
        cells = ['pinned callback', ledger.pin_link(dict(nodes[key], line=line)), prototype, target, paths[key][0], 'Sole actual address binding of the complete private record callback type; C roots accept no callback input', 'PINNED', labels_text(executed.get(target, set())), ledger.pin_link(nodes[target])]
        result.append(cells)
    boundary_callers = {}
    for caller, symbol in sorted(state['external']):
        if caller in paths: boundary_callers.setdefault(symbol, caller)
    for symbol, caller in sorted(boundary_callers.items()):
        cells = ['external boundary', ledger.pin_link(nodes[caller]), symbol, 'LLVM-symbol:' + symbol, paths[caller][0], ledger.format_path(nodes, paths[caller][1]), 'BOUNDARY', '', 'Declaration-only target outside pin definitions; no std replacement inferred from its name.']
        result.append(cells)
    return result


def projections(ledger, state, inventory):
    """Count the expected compiler projections, retaining duplicate row multiplicity."""
    rows = [function_cells(ledger, state, key) for key in inventory]
    # Boundary explanation/type leads and resolution status are compiler owned.
    rows += [compiler_projection(row) for row in boundary_cells(ledger, state)]
    return Counter(tuple(row) for row in rows)


def compiler_projection(row):
    """Extract static fields from a table row, excluding behavioral and review-delta text."""
    if row[0].startswith('function/'):
        return row[:6]
    reason = row[8].split('; Review delta from ', 1)[0]
    return row[:6] + ([] if row[0] == 'indirect' else [row[6]]) + [reason]


def validate(ledger, state, inventory, rows):
    """Compare rendered compiler cells with fresh projections and return the row count.

    Raise RuntimeError for missing or extra projections, including multiplicity.
    """
    expected = projections(ledger, state, inventory)
    actual = Counter(tuple(compiler_projection(row)) for row in rows)
    missing, extra = expected - actual, actual - expected
    if missing or extra:
        raise RuntimeError('compiler-derived ledger cells differ: missing=' +
                           repr(list(missing.elements())[:5]) + '; extra=' +
                           repr(list(extra.elements())[:5]))
    return len(rows)
