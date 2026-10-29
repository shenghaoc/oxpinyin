"""Case selection for the contract differential (contract-diff.py).

Resolve --cases, --issue, --family and --cases-prefix to case IDs and the reasons each was
chosen, build the tag index --list-tags prints, and word the plan that precedes
a run. The registry is a parameter: contract-diff.py passes its own CASES when
it calls, so a registry the tests swap in is the one selected from, and every
load of the driver keeps its own. The driver re-exports the names that read no
registry and keeps same-signature adapters for those that do.
"""
import re


def normal_tag(tag):
    """Normalize an issue selector by stripping whitespace and leading hashes.

    Lowercase the result and canonicalize a leading "Row 54" or "row:54"
    to "row:54"; numeric issue IDs and lane tags remain distinct."""
    tag = str(tag).strip().lstrip('#').lower()
    return re.sub(r'^row[ :]*', 'row:', tag)


def family_of(name):
    """Return the prefix before the first slash, or None if there is no slash."""
    return name.split('/', 1)[0] if '/' in name else None


def tag_index(registry):
    """issue tag -> sorted case IDs, and 'family/<F>' -> IDs; axis C excluded."""
    index = {}
    for name, spec in registry.items():
        if name.startswith('axis-c-'):
            continue
        for tag in spec['issues']:
            index.setdefault(tag, []).append(name)
        family = family_of(name)
        if family:
            index.setdefault('family/' + family, []).append(name)
    return {tag: sorted(names) for tag, names in sorted(index.items())}


class SelectionError(ValueError):
    """Report an unknown selector or a selection with no case IDs."""
    pass


def case_names(value):
    """Split comma-separated case IDs without trimming or deduplicating them.

    Raise SelectionError if any component is empty or whitespace-only."""
    names = value.split(',')
    if any(not name.strip() for name in names):
        raise SelectionError('--cases must not contain an empty value')
    return names


def prefix_matches(registry, value):
    """Map full-registry case IDs to reasons for comma-separated ID prefixes.

    Preserve prefix order and sort each prefix's matches. Every prefix must
    be nonempty and match at least one ID, even if another selector matches.
    """
    if value is None:
        return {}
    prefixes = value.split(',')
    if any(not prefix.strip() for prefix in prefixes):
        raise SelectionError('--cases-prefix must not contain an empty value')
    chosen = {}
    for prefix in prefixes:
        names = sorted(name for name in registry if name.startswith(prefix))
        if not names:
            raise SelectionError('--cases-prefix matched no cases: %r' % prefix)
        for name in names:
            chosen.setdefault(name, []).append('prefix ' + prefix)
    return chosen


def select_cases(registry, cases=None, issues=(), families=(), prefixes=None):
    """Return case IDs in selection order and the reasons accumulated per ID.

    Process explicit IDs first, then issue selectors, families and prefixes;
    each issue, family or prefix contributes sorted IDs. Keep the first occurrence's
    position and append all reasons, including repeated selectors. With no
    selectors, choose every non-axis-C case in sorted order. Axis C can be
    selected explicitly or by prefix but is absent from issue and family indexes.
    Raise SelectionError for unknown selectors, empty comma components or
    an empty selection; never fall back to the default suite on an error."""
    index = tag_index(registry)
    chosen = {}
    for name in case_names(cases) if cases is not None else ():
        if name not in registry:
            raise SelectionError('unknown case: %s' % name)
        chosen.setdefault(name, []).append('--cases')
    for issue in issues:
        tag = normal_tag(issue)
        if tag not in index or tag.startswith('family/'):
            known = ', '.join(t for t in index if not t.startswith('family/'))
            raise SelectionError('unknown issue tag %r; registered tags: %s' % (issue, known))
        for name in index[tag]:
            chosen.setdefault(name, []).append('issue ' + tag)
    for family in families:
        key = 'family/' + family
        if key not in index:
            known = ', '.join(t[len('family/'):] for t in index if t.startswith('family/'))
            raise SelectionError('unknown family %r; registered families: %s' % (family, known))
        for name in index[key]:
            chosen.setdefault(name, []).append('family ' + family)
    for name, reasons in prefix_matches(registry, prefixes).items():
        chosen.setdefault(name, []).extend(reasons)
    if cases is None and not (issues or families) and prefixes is None:
        chosen = {n: ['default suite'] for n in sorted(n for n in registry if not n.startswith('axis-c-'))}
    if not chosen:
        raise SelectionError('no cases selected')
    return list(chosen), chosen


def plan_lines(registry, names, reasons, pinyin_so, zhuyin_so, prefix, cell):
    """The selection plan: IDs and reasons (one line each for an explicit
    selection; a count for the default suite), facades and artifacts."""
    modes = {}
    for name in names:
        modes[registry[name]['mode']] = modes.get(registry[name]['mode'], 0) + 1
    skipped = sum(1 for name in names if cell not in registry[name]['cells'])
    explicit = any(reason != ['default suite'] for reason in reasons.values())
    lines = ['plan: %d case%s%s; cell %s; facades: %s' % (
        len(names), '' if len(names) == 1 else 's',
        '' if explicit else ' (default suite: every registered non-axis-C case)', cell,
        ', '.join('%s %d' % item for item in sorted(modes.items())))]
    if skipped:
        lines.append('  %d case%s skip on this cell (fixture in another backend\'s format)' % (
            skipped, '' if skipped == 1 else 's'))
    artifacts = ['subject %s' % pinyin_so, 'pin %s' % (prefix / 'lib/libpinyin.so'),
                 'system tables %s' % (prefix / 'lib/libpinyin/data')]
    if 'zhuyin' in modes:
        artifacts += ['subject %s' % (zhuyin_so or '--zhuyin-so (missing)'), 'pin %s' % (prefix / 'lib/libzhuyin.so')]
    lines.append('  artifacts: ' + '; '.join(artifacts))
    if explicit:
        width = max(len(name) for name in names)
        for name in names:
            lines.append('  %-*s  %-7s %s' % (width, name, registry[name]['mode'], ', '.join(reasons[name])))
    return lines
