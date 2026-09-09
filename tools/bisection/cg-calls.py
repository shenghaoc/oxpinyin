"""cg-calls.py -- callgrind call counts keyed on the callee's identity.

  cg-calls.py <callgrind.out> calls   <name substring>   calls TO each match
  cg-calls.py <callgrind.out> callers <name substring>   who calls them

Written for the RSS diagnosis (docs/findings/rss-attribution-2026-09-09.md),
where valgrind's stack unwind through the C-ABI boundary could not be
trusted and call edges were the only sound attribution. See
docs/runbooks/benches.md, "Attributing a heap gap to callers", for the two
rules this encodes and the numbers that were wrong before it did.

Identity is the callee's (object, file, name) triple.

Callgrind's name-compression IDs are per name-space (ob / fl / fn), and two
distinct functions can share a demangled name -- an .isra/.constprop clone,
or the same method in two objects. Keying on the name alone silently merges
them, which is how an earlier pass produced two different totals for
BasicDB::get. Identity here is the triple, as callgrind itself uses.
"""
import re,sys,collections

def parse(path):
    obn,fln,fnn={},{},{}
    cur_ob=cur_fl=cur_fn=None
    c_ob=c_fl=c_fn=None
    edges=collections.Counter()
    def name(tbl,num,txt):
        if txt: tbl[num]=txt
        return tbl.get(num,f'?{num}')
    for line in open(path):
        line=line.rstrip('\n')
        m=re.match(r'^(c?)(ob|fl|fi|fe|fn)=(?:\((\d+)\))?\s*(.*)$',line)
        if m:
            pre,kind,num,txt=m.group(1),m.group(2),m.group(3),m.group(4).strip()
            if kind in ('fi','fe'): kind='fl'
            tbl={'ob':obn,'fl':fln,'fn':fnn}[kind]
            val=name(tbl,num,txt) if num else txt
            if pre=='c':
                if kind=='ob': c_ob=val
                elif kind=='fl': c_fl=val
                else: c_fn=val
            else:
                if kind=='ob': cur_ob=val; c_ob=None
                elif kind=='fl': cur_fl=val
                else: cur_fn=val; c_fn=None
            continue
        m=re.match(r'^calls=(\d+)',line)
        if m and c_fn is not None:
            callee=(c_ob or cur_ob, c_fl or cur_fl, c_fn)
            caller=(cur_ob, cur_fl, cur_fn)
            edges[(caller,callee)]+=int(m.group(1))
            c_fn=None; c_ob=None; c_fl=None
    return edges

if len(sys.argv) != 4 or sys.argv[2] not in ('calls', 'callers'):
    print(__doc__, file=sys.stderr)
    raise SystemExit(2)

edges=parse(sys.argv[1])
mode=sys.argv[2]           # 'calls' or 'callers'
needle=sys.argv[3].lower()
if mode=='calls':
    tot=collections.Counter()
    for (caller,callee),n in edges.items():
        if needle in callee[2].lower(): tot[callee]+=n
    print(f'== {sys.argv[1].rsplit("/",1)[-1]}  calls TO *{sys.argv[3]}*')
    for (ob,fl,fn),n in tot.most_common(14):
        print(f'{n:>9,}  {fn[:96]}')
        print(f'{"":>9}     in {(ob or "?").rsplit("/",1)[-1]} :: {(fl or "?").rsplit("/",1)[-1]}')
else:
    tot=collections.Counter()
    for (caller,callee),n in edges.items():
        if needle in callee[2].lower(): tot[caller]+=n
    print(f'== {sys.argv[1].rsplit("/",1)[-1]}  callers of *{sys.argv[3]}*')
    for (ob,fl,fn),n in tot.most_common(12):
        print(f'{n:>9,}  {fn[:100]}')
    print(f'{sum(tot.values()):>9,}  TOTAL')
