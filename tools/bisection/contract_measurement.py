"""Per-call measurement for contract-diff, with driver state passed at the boundary.

PerCallKit is a mixin: the driver combines it with its own Kit, retaining the
existing loader and method patch points. The controller and worker receive the
registry, callbacks and driver path from their same-signature driver adapters.
"""
import contextlib
import ctypes as C
import hashlib
import json
import os
from pathlib import Path
import re
import resource
import statistics
import subprocess
import sys
import tempfile
import time

from contract_abi import B, I, P, S, U, Arr


PER_CALL_OPS = ('add', 'count', 'lookup')
PER_CALL_NAMES = dict(zip(PER_CALL_OPS, ('iterator_add_phrase', 'token_get_n_pronunciation', 'lookup_tokens')))
PER_CALL_NATIVE = r'''
#define _POSIX_C_SOURCE 200809L
#include <stdbool.h>
#include <stdint.h>
#include <time.h>
#include <valgrind/callgrind.h>
#if !defined(__x86_64__)
#error "per-call calibration currently requires x86-64"
#endif
static uint64_t calls[3], nanos[3];
static int profile, started;
void pc_stop(void) { if (started) { CALLGRIND_STOP_INSTRUMENTATION; started = 0; } }
void pc_start(void) { if (profile && !started) { CALLGRIND_START_INSTRUMENTATION; started = 1; } }
void pc_reset(int p) { pc_stop(); profile = p; for (int i=0; i<3; i++) calls[i] = nanos[i] = 0; }
uint64_t pc_calls(int i) { return calls[i]; }
uint64_t pc_nanos(int i) { return nanos[i]; }
static uint64_t now(void) {
    struct timespec t;
    if (clock_gettime(CLOCK_MONOTONIC_RAW, &t)) __builtin_trap();
    return (uint64_t)t.tv_sec * 1000000000 + t.tv_nsec;
}
typedef bool (*add_fn)(void *, const char *, const char *, int);
typedef bool (*count_fn)(void *, unsigned, unsigned *);
typedef bool (*lookup_fn)(void *, const char *, void *);
#define MEASURE(INDEX, EXPR) \
    bool ret; \
    if (profile) { \
        CALLGRIND_TOGGLE_COLLECT; \
        ret = (EXPR); \
        CALLGRIND_TOGGLE_COLLECT; \
    } else { \
        uint64_t begin = now(); \
        ret = (EXPR); \
        uint64_t end = now(); \
        nanos[INDEX] += end - begin; \
    } \
    calls[INDEX]++; \
    return ret
bool pc_add(add_fn f, void *p, const char *s, const char *r, int n) { MEASURE(0, f(p,s,r,n)); }
bool pc_count(count_fn f, void *p, unsigned t, unsigned *n) { MEASURE(1, f(p,t,n)); }
bool pc_lookup(lookup_fn f, void *p, const char *s, void *a) { MEASURE(2, f(p,s,a)); }
/* Each callee has exactly three instructions. The driver checks disassembly.
 * The caller uses the same ABI and client requests as the real call. */
__asm__(".text\n.globl pc_noop\n.type pc_noop,@function\npc_noop:\nmov $1,%eax\nnop\nret\n.size pc_noop,.-pc_noop");
extern bool pc_noop_add(void *, const char *, const char *, int) __asm__("pc_noop");
extern bool pc_noop_count(void *, unsigned, unsigned *) __asm__("pc_noop");
extern bool pc_noop_lookup(void *, const char *, void *) __asm__("pc_noop");
void pc_calibrate(int op, unsigned n) {
    pc_start();
    for (unsigned i=0; i<n; i++) {
        if (op == 0) pc_add(pc_noop_add, 0, 0, 0, 0);
        if (op == 1) pc_count(pc_noop_count, 0, 0, 0);
        if (op == 2) pc_lookup(pc_noop_lookup, 0, 0, 0);
    }
    pc_stop();
}
bool pc_lookup_loop(lookup_fn f, void *p, const char *s, void *a, unsigned n) {
    bool ret = true;
    pc_start();
    for (unsigned i=0; i<n; i++) ret &= pc_lookup(f,p,s,a);
    pc_stop();
    return ret;
}
'''


def per_call_helper(scratch, *, PER_CALL_NATIVE):
    """Compile the native per-call helper and verify its calibration instruction prefix.

    Write calls.c and calls.so under scratch, invoking cc and objdump. Return the
    library path; raise ValueError on compilation failure or when pc_noop does
    not start with mov, nop, ret. Subprocess launch/read errors propagate.
    """
    source, library = Path(scratch) / 'calls.c', Path(scratch) / 'calls.so'
    source.write_text(PER_CALL_NATIVE)
    compiled = subprocess.run(['cc', '-O2', '-shared', '-fPIC', '-fcf-protection=none',
                               '-Wall', '-Wextra', '-Werror', '-o', str(library), str(source)],
                              capture_output=True, text=True)
    if compiled.returncode:
        raise ValueError('per-call helper compile failed: ' + compiled.stderr)
    assembly = subprocess.check_output(['objdump', '-d', '--disassemble=pc_noop', str(library)], text=True)
    instructions = re.findall(r'^\s*[0-9a-f]+:\s+(?:[0-9a-f]{2}\s+)+\s*(\S+)', assembly, re.M)
    # GCC can put alignment bytes or an unreachable ud2 after the return.
    if instructions[:3] != ['mov', 'nop', 'ret']:
        raise ValueError('calibration callee must be mov, nop, ret: ' + repr(instructions))
    return library


def per_call_native(library, profile):
    """Load the per-call helper, bind its ctypes signatures and reset its counters.

    profile selects instruction collection rather than native wall timing;
    return the configured CDLL, propagating load or symbol errors.
    """
    native = C.CDLL(str(library))
    for name in ('pc_start', 'pc_stop'):
        getattr(native, name).argtypes, getattr(native, name).restype = [], None
    native.pc_reset.argtypes, native.pc_reset.restype = [I], None
    native.pc_calibrate.argtypes, native.pc_calibrate.restype = [I, U], None
    for name in ('pc_calls', 'pc_nanos'):
        getattr(native, name).argtypes, getattr(native, name).restype = [I], C.c_uint64
    for op, args in (('add', [P, S, S, I]), ('count', [P, U, C.POINTER(U)]), ('lookup', [P, S, P])):
        f = getattr(native, 'pc_' + op)
        f.argtypes, f.restype = [P] + args, B
    native.pc_lookup_loop.argtypes, native.pc_lookup_loop.restype = [P, P, S, P, U], B
    native.pc_reset(profile)
    return native


class PerCallKit:
    """Wrap selected C API calls for measurement inside explicit per-call steps.

    Unselected calls keep their original bindings. Selected calls use the native
    wrapper only while active; outside a step they stop collection and call the
    original function.
    """
    def __init__(self, mode, so, data, scratch, native, operations):
        """Initialize ordinary kit state and retain the native helper and selected operations.

        Measurement begins inactive; operations names are resolved through
        PER_CALL_NAMES when a function binding is requested.
        """
        super().__init__(mode, so, data, scratch)
        self.native, self.operations, self.active = native, operations, False

    @contextlib.contextmanager
    def per_call_step(self):
        """Start native collection and mark calls active until the context exits.

        The finally block clears the active flag; it does not itself call pc_stop.
        """
        self.active = True
        self.native.pc_start()
        try:
            yield
        finally:
            self.active = False

    def fn(self, name, res, *args):
        """Bind an API function and conditionally wrap selected measurement operations.

        Requesting an unselected binding while inactive stops collection immediately;
        selected bindings defer the active-state check until invocation.
        """
        original = super().fn(name, res, *args)
        operation = next((op for op in self.operations if PER_CALL_NAMES[op] == name), None)
        if operation is None:
            if not self.active:
                self.native.pc_stop()
            return original
        wrapped = getattr(self.native, 'pc_' + operation)
        def call(*values):
            """Measure the original call while active, otherwise stop collection and call directly."""
            if self.active:
                return wrapped(C.cast(original, P), *values)
            self.native.pc_stop()
            return original(*values)
        return call


def per_call_worker(mode, so, data, name, library, metric, operations, *, PerCallKit, Kit, CASES, per_call_native):
    """Execute one per-call workload and print its observation and native counters as JSON.

    Disable core dumps. Calibration makes 1000 calls; lookup-only makes 10000
    native-loop lookups; other names invoke registered cases with a PerCallKit
    using TMPDIR. Stop collection before reporting calls and nanoseconds per
    operation; failures propagate to the worker process.
    """
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    native = per_call_native(library, metric == 'ir')
    if name == 'calibrate':
        native.pc_calibrate(PER_CALL_OPS.index(operations), 1000)
        result = {'calibration': True}
    else:
        kit = PerCallKit(mode, so, data, os.environ['TMPDIR'], native, operations.split(','))
        if name == 'lookup-only':
            # No add occurs. All iterations run in C and reuse one GArray.
            new = kit.glib.g_array_new
            new.argtypes, new.restype = [I, I, U], P
            array = new(0, 0, 4)
            lookup = Kit.fn(kit, 'lookup_tokens', B, P, S, P)
            ret = native.pc_lookup_loop(C.cast(lookup, P), kit.inst, '你好'.encode(), array, 10000)
            view = C.cast(array, C.POINTER(Arr)).contents
            result = dict(ret=ret, tokens=C.cast(view.data, C.POINTER(U))[:view.len])
            kit.glib.g_array_free.argtypes, kit.glib.g_array_free.restype = [P, I], P
            kit.glib.g_array_free(array, True)
        else:
            result = CASES[name]['fn'](kit)
        result['logs'] = kit.logs
    native.pc_stop()
    stats = {op: dict(calls=native.pc_calls(i), ns=native.pc_nanos(i)) for i, op in enumerate(PER_CALL_OPS)}
    print(json.dumps(dict(observation=result, stats=stats), ensure_ascii=True), flush=True)


def per_call_ir(stderr):
    """Extract one positive Ir total from Callgrind text.

    Require exactly one events line containing only Ir and one decimal totals
    line; raise ValueError for missing, duplicate, mixed or nonpositive totals.
    """
    events = re.findall(r'^events:\s*(.*?)\s*$', stderr, re.M)
    totals = re.findall(r'^totals:\s*(.*?)\s*$', stderr, re.M)
    if events != ['Ir'] or len(totals) != 1 or not re.fullmatch(r'\d+', totals[0]):
        raise ValueError('expected one Callgrind Ir events/totals pair')
    value = int(totals[0])
    if value <= 0:
        raise ValueError('Callgrind collected no instructions')
    return value


def per_call_profile(stderr):
    """Read exclusive function costs and inclusive call edges, in memory.
    Callgrind name IDs share object/file/function namespaces with their
    callee forms. Position columns can use relative compression; this reader
    needs their width, not their values. Costs stay raw and additive only
    within the exclusive function table.
    """
    total = per_call_ir(stderr)
    aliases = {'ob': 'ob', 'cob': 'ob', 'fn': 'fn', 'cfn': 'fn',
               'fl': 'fl', 'fi': 'fl', 'fe': 'fl', 'cfi': 'fl', 'cfl': 'fl'}
    names = {key: {} for key in ('ob', 'fn', 'fl')}
    lines = stderr.splitlines()
    for line in lines:
        match = re.match(r'^(\w+)=(.*)$', line)
        if match and match[1] in aliases:
            compressed = re.fullmatch(r'\((\d+)\)(?: (.*))?', match[2])
            if compressed and compressed[2] is not None:
                table = names[aliases[match[1]]]
                if compressed[1] in table and table[compressed[1]] != compressed[2]:
                    raise ValueError('conflicting Callgrind name ID')
                table[compressed[1]] = compressed[2]

    def resolve(kind, value):
        """Resolve a compressed Callgrind name using the shared namespace table.

        Return uncompressed names unchanged and raise ValueError for unresolved IDs.
        """
        compressed = re.fullmatch(r'\((\d+)\)(?: (.*))?', value)
        if not compressed:
            return value
        try:
            return names[aliases[kind]][compressed[1]]
        except KeyError:
            raise ValueError('unresolved Callgrind name ID: ' + kind + '=' + value) from None

    positions = 1
    obj, function = '???', '???'
    next_callee_obj, callee = None, None
    pending = None
    exclusive, edges = {}, {}
    for line in lines:
        if line.startswith('positions:'):
            positions = len(line.split(':', 1)[1].split())
            if not 1 <= positions <= 2:
                raise ValueError('unsupported Callgrind position columns')
        match = re.match(r'^(\w+)=(.*)$', line)
        if match and match[1] in aliases:
            kind, value = match[1], resolve(match[1], match[2])
            if kind == 'ob':
                obj = value
            elif kind == 'fn':
                function, next_callee_obj, callee = value, None, None
            elif kind == 'cob':
                next_callee_obj = value
            elif kind == 'cfn':
                callee = (next_callee_obj if next_callee_obj is not None else obj, value)
                next_callee_obj = None
            continue
        if line.startswith('calls='):
            fields = line.removeprefix('calls=').split()
            if pending is not None or callee is None or not fields or not fields[0].isdigit():
                raise ValueError('invalid Callgrind call association')
            pending = ((obj, function), callee, int(fields[0]))
            continue
        if line.startswith(('jump=', 'jcnd=')):
            raise ValueError('jump associations are not supported by per-call attribution')
        if not re.match(r'^(?:0x[0-9a-fA-F]+|\d+|[+\-]\d+|\*)(?:\s|$)', line):
            continue
        fields = line.split()
        costs = fields[positions:]
        if len(fields) < positions or len(costs) > 1 or (costs and not costs[0].isdigit()):
            raise ValueError('invalid Callgrind Ir cost line')
        cost = int(costs[0]) if costs else 0
        if pending is None:
            key = (obj, function)
            exclusive[key] = exclusive.get(key, 0) + cost
        else:
            caller, target, count = pending
            old_cost, old_count = edges.get((caller, target), (0, 0))
            edges[caller, target] = (old_cost + cost, old_count + count)
            pending = None
    if pending is not None or sum(exclusive.values()) != total:
        raise ValueError('Callgrind exclusive costs do not reconcile with totals')
    return dict(total=total, exclusive=exclusive, edges=edges)


def per_call_attribution(profile, calls, limit):
    """Named roots are evidence, not an invented partition of an inlined API.
    Inclusive edges can overlap. Never sum them as exclusive phase costs.
    """
    def identity(key):
        """Represent an (object, function) profile key as named JSON fields."""
        return dict(object=key[0], function=key[1])
    functions = [dict(**identity(key), ir_per_call=value / calls)
                 for key, value in sorted(profile['exclusive'].items(), key=lambda item: (-item[1], item[0]))
                 if value]
    edges = [dict(caller=identity(caller), callee=identity(callee), inclusive_ir_per_call=value / calls,
                  callgrind_calls_per_selected_call=count / calls)
             for (caller, callee), (value, count) in sorted(profile['edges'].items(),
                                                         key=lambda item: (-item[1][0], item[0])) if value]
    # These roots follow the current add path in both iterators.rs files.
    # A source boundary may disappear under LTO, so absence is unresolved.
    patterns = dict(lookup=r'phrase_table_tokens|SystemDictionary.*tokens_for_text',
                    validation=r'RuntimeDict.*token_introspection|GenericUserStore.*::phrase$',
                    store_merge=r'GenericUserStore.*::add_(?:system_)?phrase_in$')
    candidates = {}
    for phase, pattern in patterns.items():
        selected = [edge for edge in edges if re.search(pattern, edge['callee']['function'])]
        candidates[phase] = dict(status='named_roots_only' if selected else 'unresolved',
                                 inclusive_edges=selected)
    return dict(basis='raw_collected_Ir', raw_ir_per_call=profile['total'] / calls,
                exclusive_sum_ir_per_call=sum(profile['exclusive'].values()) / calls,
                inclusive_edges_overlap=True, phase_partition='not_established',
                edge_call_counts='Callgrind records; can include calls outside collection',
                named_phase_candidates=candidates, functions=functions[:limit] if limit else functions,
                call_edges=edges[:limit] if limit else edges,
                omitted_functions=max(0, len(functions) - limit) if limit else 0,
                omitted_call_edges=max(0, len(edges) - limit) if limit else 0)


def per_call_run(mode, so, data, name, helper, metric, operations, scratch, cpu, attribution=False, *,
                 driver_file, CELL, check_tkrzw, check_cancelled, terminate_group, user_dir_listing,
                 per_call_ir, per_call_profile):
    """Run one CPU-pinned measurement worker in a disposable sample directory.

    Use Callgrind for metric 'ir', optionally retaining an in-memory parsed
    profile. Check cancellation while waiting up to 900 seconds and terminate
    the process group on interruption or timeout. Require one JSON document and
    a zero exit; return observations, native counters and optional instruction
    data before removing the directory.
    """
    with tempfile.TemporaryDirectory(prefix='sample-', dir=scratch) as sample:
        command = ['taskset', '-c', str(cpu)]
        if metric == 'ir':
            command += ['valgrind', '--tool=callgrind', '--instr-atstart=no', '--collect-atstart=no',
                        '--cache-sim=no', '--branch-sim=no', '--callgrind-out-file=/dev/stderr']
            if attribution:
                command += ['--read-inline-info=yes', '--demangle=yes']
        command += [sys.executable, driver_file, '--per-call-worker', mode, str(so), str(data), name,
                    str(helper), metric, operations]
        env = dict(os.environ, TMPDIR=sample)
        if name != 'calibrate':
            check_tkrzw(env.get('CONTRACT_DIFF_CELL', CELL), 'contract-diff.py/per_call',
                        env.get('OXPINYIN_RUNTIME_SIDE', 'subject'), so, env=env)
        # Captures and profiler output never reach a file.
        check_cancelled()
        with subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env,
                              start_new_session=True) as proc:
            deadline = time.monotonic() + 900
            stdout, stderr = b'', b''
            try:
                while True:
                    check_cancelled()
                    remaining = deadline - time.monotonic()
                    if remaining <= 0:
                        raise subprocess.TimeoutExpired(command, 900, output=stdout, stderr=stderr)
                    try:
                        stdout, stderr = proc.communicate(timeout=min(0.1, remaining))
                    except subprocess.TimeoutExpired as error:
                        stdout, stderr = error.output or b'', error.stderr or b''
                        continue
                    check_cancelled()
                    break
            except BaseException:
                terminate_group(proc)
                raise
        check_cancelled()
        stderr = stderr.decode('utf-8', 'surrogateescape')
        documents = [json.loads(line) for line in stdout.decode().splitlines() if line.startswith('{')]
        if proc.returncode or len(documents) != 1:
            raise ValueError('per-call worker failed: exit %s, %s' % (proc.returncode, stderr[-3000:]))
        document = documents[0]
        observation = dict(exit=0, result=document['observation'], stderr=stderr,
                           stderr_lines=len(stderr.splitlines()), userfiles=user_dir_listing(sample, set()))
        return dict(observation=observation, stats=document['stats'],
                    ir=per_call_ir(stderr) if metric == 'ir' else None,
                    profile=per_call_profile(stderr) if metric == 'ir' and attribution else None)


def per_call_checked(spec, reference, sample, operations, *, verdict_for):
    """Require a matching observation and positive, equal attempt counts for each operation.

    Use the normal verdict evaluator with parent expectations disabled; raise
    ValueError on any non-MATCH verdict, unexpected result or count discrepancy.
    """
    verdict, expected, same = verdict_for(spec, reference['observation'], sample['observation'], False)
    if verdict != 'MATCH' or not expected or not same:
        raise ValueError('per-call observation failed: ' + verdict)
    for op in operations:
        count = sample['stats'][op]['calls']
        if count <= 0 or count != reference['stats'][op]['calls']:
            raise ValueError('per-call attempt count differs for ' + op)


def per_call_calibration(total, calls=1000):
    """Return wrapper instructions per call after subtracting the three-instruction callee.

    Require exactly 1000 calls, a positive integral per-call total exceeding three
    instructions, and raise ValueError if calibration violates those constraints.
    """
    if calls != 1000 or total % calls or total <= 3 * calls:
        raise ValueError('invalid three-instruction callee calibration')
    return total // calls - 3


def run_per_call(args, parser, *, CASES, FUNCTION_ORDER, per_call_helper, per_call_run,
                 per_call_checked, per_call_calibration, per_call_attribution, observed):
    """Validate and run the selected per-call comparison, emitting JSON measurements.

    Reject incompatible selectors, invalid operations, missing libraries and a
    CPU outside the current affinity via parser.error. Compile a temporary helper,
    calibrate wrapper Ir, compare three rotated wall rounds and per-operation Ir,
    and require matching observations/counts. Print nanoseconds and calibrated
    instructions per call, ratios to the pin and optional attribution; return
    zero on success and propagate measurement failures.
    """
    if args.cases or args.issue or args.family or args.expect_parent or args.observations or args.plan_only or \
            args.regenerate_sentence_ties or args.axis_c_replacement or args.axis_c_mutation or args.axis_c_snapshot_check:
        parser.error('--per-call cannot be combined with contract selectors, captures or other modes')
    operations = tuple(args.per_call_operations.split(',')) if args.per_call_operations else (
        PER_CALL_OPS if args.per_call == 'wrap' else ('lookup',))
    if not operations or len(set(operations)) != len(operations) or any(op not in PER_CALL_OPS for op in operations):
        parser.error('--per-call-operations needs unique add,count,lookup names')
    if args.per_call == 'lookup-only' and operations != ('lookup',):
        parser.error('lookup-only measures lookup only')
    if args.per_call_attribution_limit < 0:
        parser.error('--per-call-attribution-limit must be nonnegative')
    modes = ('pinyin', 'zhuyin') if args.per_call_facades == 'both' else (args.per_call_facades,)
    sides = {'pin': {'pinyin': args.prefix / 'lib/libpinyin.so', 'zhuyin': args.prefix / 'lib/libzhuyin.so'},
             'subject': {'pinyin': args.pinyin_so, 'zhuyin': args.zhuyin_so}}
    if args.base_pinyin_so or args.base_zhuyin_so:
        sides = dict(pin=sides['pin'], base={'pinyin': args.base_pinyin_so, 'zhuyin': args.base_zhuyin_so},
                     subject=sides['subject'])
    for side, artifacts in sides.items():
        for mode in modes:
            if not artifacts[mode] or not artifacts[mode].is_file():
                parser.error('missing %s %s library' % (side, mode))
            artifacts[mode] = artifacts[mode].resolve()
    FUNCTION_ORDER['check_arms'](args, {side: {mode: paths[mode] for mode in modes}
                                      for side, paths in sides.items()}, references=('pin',))
    from tools.dev import host
    allowed = host.allowed_cpus()
    cpu = args.per_call_cpu if args.per_call_cpu is not None else min(allowed)
    if cpu not in allowed:
        parser.error('--per-call-cpu is outside this process CPU affinity')
    data = args.prefix / 'lib/libpinyin/data'
    provenance = {side: {mode: dict(path=str(paths[mode]), sha256=hashlib.sha256(paths[mode].read_bytes()).hexdigest())
                         for mode in modes} for side, paths in sides.items()}
    print(json.dumps(dict(per_call='start', workload=args.per_call, cpu=cpu, libraries=provenance,
                         wall_unit='ns/call', ir_unit='Ir/call', wall_rounds=3)), flush=True)
    with tempfile.TemporaryDirectory(prefix='contract-per-call-') as scratch:
        helper = per_call_helper(scratch)
        calibration = {}
        for op in operations:
            sample = per_call_run('pinyin', sides['pin']['pinyin'], data, 'calibrate', helper, 'ir', op, scratch, cpu)
            calibration[op] = per_call_calibration(sample['ir'], sample['stats'][op]['calls'])
        print(json.dumps(dict(per_call='calibration', wrapper_ir=calibration, callee_ir=3, calls=1000)), flush=True)
        for mode in modes:
            name = ('wrap-256th-reading' + ('-zhuyin' if mode == 'zhuyin' else '')
                    if args.per_call == 'wrap' else 'lookup-only')
            spec = CASES['wrap-256th-reading' + ('-zhuyin' if mode == 'zhuyin' else '')]
            reference, walls, counts = None, {side: {op: [] for op in operations} for side in sides}, {}
            for round_index in range(3):
                order = list(sides)
                order = order[round_index % len(order):] + order[:round_index % len(order)]
                for side in order:
                    print('per-call: %s native round %d %s' % (mode, round_index + 1, side), file=sys.stderr, flush=True)
                    sample = per_call_run(mode, sides[side][mode], data, name, helper, 'wall',
                                          ','.join(operations), scratch, cpu)
                    if reference is None:
                        reference = sample
                    per_call_checked(spec, reference, sample, operations)
                    for op in operations:
                        counts[op] = sample['stats'][op]['calls']
                        walls[side][op].append(sample['stats'][op]['ns'] / counts[op])
            irs = {side: {} for side in sides}
            profiles = {side: {} for side in sides}
            for side in sides:
                for op in operations:
                    print('per-call: %s Ir %s %s' % (mode, side, op), file=sys.stderr, flush=True)
                    sample = per_call_run(mode, sides[side][mode], data, name, helper, 'ir', op, scratch, cpu,
                                          attribution=args.per_call_attribution)
                    per_call_checked(spec, reference, sample, (op,))
                    value = sample['ir'] / counts[op] - calibration[op]
                    if value <= 0:
                        raise ValueError('nonpositive calibrated Ir for ' + op)
                    irs[side][op] = value
                    if args.per_call_attribution:
                        profiles[side][op] = per_call_attribution(
                            sample['profile'], counts[op], args.per_call_attribution_limit)
            for op in operations:
                pin_ns = statistics.median(walls['pin'][op])
                pin_ir = irs['pin'][op]
                for side in sides:
                    ns = statistics.median(walls[side][op])
                    ir = irs[side][op]
                    if args.per_call_attribution:
                        print(json.dumps(dict(per_call='attribution', facade=mode, operation=op, side=side,
                                              wrapper_ir_per_call=calibration[op],
                                              **profiles[side][op])), flush=True)
                    print(json.dumps(dict(per_call='result', workload=args.per_call, facade=mode, operation=op,
                                          side=side, calls=counts[op], wall_ns_per_call=ns, ir_per_call=ir,
                                          wall_ratio=ns / pin_ns, ir_ratio=ir / pin_ir,
                                          native_rounds_ns_per_call=walls[side][op],
                                          observation_sha256=hashlib.sha256(json.dumps(observed(
                                              reference['observation']['result']), sort_keys=True).encode()).hexdigest())),
                          flush=True)
    return 0
