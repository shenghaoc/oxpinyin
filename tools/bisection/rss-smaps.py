#!/usr/bin/env python3
"""rss-smaps.py — group a /proc/self/smaps dump by mapping, and diff two.

Written for the RSS diagnosis (docs/findings/rss-attribution-2026-09-09.md).
`bisect --perf` in PERF_MODE=rss-diag with RSS_DIAG_DIR set writes the raw
dumps; this turns them into the per-mapping Rss table the attribution is
read off, and, with --diff, into the S-minus-L delta that names which
mappings carry a gap.

  rss-smaps.py <smaps> [<smaps> ...]        one table per dump
  rss-smaps.py --diff <smaps-A> <smaps-B>   B minus A, per mapping

Mappings are grouped by identity, not by address: a library or a data file
mapped as several segments is one row, which is what the question "which
mapping carries the difference" actually asks.
"""

import collections
import os
import re
import sys

HEADER = re.compile(r'^([0-9a-f]+)-([0-9a-f]+) (\S+) \S+ \S+ \S+\s*(.*)$')
FIELDS = ('Rss', 'Anonymous', 'Private_Dirty', 'Private_Clean',
          'Shared_Clean', 'Size')


def parse(path):
    """Every mapping in one smaps dump, as dicts of the fields we read."""
    rows = []
    cur = None
    with open(path) as handle:
        for line in handle:
            match = HEADER.match(line)
            if match:
                cur = {'perms': match.group(3), 'name': match.group(4).strip()}
                cur.update({f: 0 for f in FIELDS})
                rows.append(cur)
                continue
            if cur is None:
                continue
            key, _, value = line.partition(':')
            if key in cur and value.strip().endswith('kB'):
                cur[key] = int(value.strip().split()[0])
    return rows


def group_key(row):
    """Collapse a mapping to its identity.

    DATA: is libpinyin's installed data directory, the structures both
    engines open; CODE: is any shared object. Keeping the two prefixes
    apart is the point: a gap in DATA: is a paging-pattern difference over
    the same files, a gap in CODE: is code residency, and a gap in [heap]
    is the allocator.
    """
    name = row['name']
    if not name:
        return '[anon]'
    base = os.path.basename(name)
    if '/libpinyin/data/' in name:
        return 'DATA:' + base
    if name.endswith('.so') or '.so.' in base:
        return 'CODE:' + base
    return name


def totals(path):
    out = collections.Counter()
    for row in parse(path):
        out[group_key(row)] += row['Rss']
    return out


def table(path):
    rows = parse(path)
    agg = {}
    for row in rows:
        acc = agg.setdefault(group_key(row), collections.Counter())
        for field in FIELDS:
            acc[field] += row[field]
    total = sum(a['Rss'] for a in agg.values())
    print(f'== {path}   total Rss = {total} kB')
    print(f'{"mapping":<44}{"Rss":>9}{"Anon":>9}{"PrivD":>9}'
          f'{"PrivC":>9}{"ShrC":>9}')
    for name, acc in sorted(agg.items(), key=lambda kv: -kv[1]['Rss']):
        if acc['Rss'] == 0:
            continue
        print(f'{name:<44}{acc["Rss"]:>9}{acc["Anonymous"]:>9}'
              f'{acc["Private_Dirty"]:>9}{acc["Private_Clean"]:>9}'
              f'{acc["Shared_Clean"]:>9}')
    print()


def diff(path_a, path_b):
    a, b = totals(path_a), totals(path_b)
    print(f'{"mapping":<44}{"A":>9}{"B":>9}{"B-A":>9}')
    for name in sorted(set(a) | set(b), key=lambda k: -abs(b[k] - a[k])):
        print(f'{name:<44}{a[name]:>9}{b[name]:>9}{b[name] - a[name]:>+9}')
    print(f'{"TOTAL":<44}{sum(a.values()):>9}{sum(b.values()):>9}'
          f'{sum(b.values()) - sum(a.values()):>+9}')


def main(argv):
    if len(argv) > 1 and argv[0] == '--diff':
        if len(argv) != 3:
            print(__doc__, file=sys.stderr)
            return 2
        diff(argv[1], argv[2])
        return 0
    if not argv:
        print(__doc__, file=sys.stderr)
        return 2
    for path in argv:
        table(path)
    return 0


if __name__ == '__main__':
    # Restore the default SIGPIPE disposition so `| head` ends this quietly
    # instead of raising BrokenPipeError out of a print.
    import signal
    signal.signal(signal.SIGPIPE, signal.SIG_DFL)
    sys.exit(main(sys.argv[1:]))
