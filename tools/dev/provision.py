"""Cloud provisioning and the shell-readable oracle environment.

Run from the repository root after `uv python install`:
`uv run --no-project --managed-python python -X utf8 -m tools.dev.provision`.
uv enforces its own floor (uv.toml required-version). Compatibility identity,
loader, cancellable process and bounded pool services remain their sole owners.
Reference images receive tools/oracle/build.py and its recipe inputs, never tools.dev.
"""

if __name__ == '__main__':
    from .entry import require_utf8_mode

    require_utf8_mode()

from . import cell as backend_cell
from contextlib import ExitStack
import hashlib
import os
from pathlib import Path
import platform
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import time

from . import doctor, model, slot
from .process import ROOT, capture, fail, run, text
from .runners import Finished
from tools import cachedir
from tools.oracle import image as images
from tools.oracle.image import digest_file, runtime_command
from tools.tkrzw import runtime
from . import vcpkg

CELLS = backend_cell.CELLS
# The first line of every environment file this module writes. A file without it came from
# the retired shell setup and is regenerated, never parsed.
ENV_HEADER = '# Written by tools.dev.provision; source it from sh, dash, bash or zsh; do not edit it.'
_EXPORT = re.compile(r"export ([A-Za-z_][A-Za-z_0-9]*)=((?:'[^']*'|\\')+)")
_QUOTED = re.compile(r"'([^']*)'|\\(')")


def quote(value):
    """Single-quote VALUE as one POSIX shell word; refuse control characters.

    The environment file holds paths and cell names, so no value needs ANSI-C quoting."""
    value = str(value)
    if any(ord(char) < 32 or ord(char) == 127 for char in value):
        raise ValueError(f'environment file value has a control character: {value!r}')
    return "'" + value.replace("'", "'\\''") + "'"


def read_env(path):
    """Return the single-quoted exports of an environment file this module wrote.

    Only lines that start `export NAME='...'` count; the first assignment of a name wins.
    An absent file, or one without ENV_HEADER, yields {}."""
    path = Path(path)
    if not path.is_file():
        return {}
    lines = path.read_text().splitlines()
    if lines[:1] != [ENV_HEADER]:
        return {}
    values = {}
    for line in lines[1:]:
        match = _EXPORT.match(line)
        if match:
            values.setdefault(match[1], ''.join(a or b for a, b in _QUOTED.findall(match[2])))
    return values


def check_manifest(manifest, cwd):
    """`sha256sum --check --quiet MANIFEST` in CWD, in Python: answer the status and the output.

    Each line is `HEX  NAME` (or `HEX *NAME`). A matching file prints nothing; a mismatch prints
    `NAME: FAILED`, and an unreadable file `sha256sum: NAME: REASON` then `NAME: FAILED open or
    read`. The counts follow as sha256sum's WARNING lines. A manifest with no well-formed line
    fails with sha256sum's message. Status 1 on any failure, as sha256sum."""
    lines, failed, unread, improper, proper = [], 0, 0, 0, 0
    try:
        entries = (Path(cwd) / manifest).read_text().splitlines()
    except OSError as error:
        return 1, f'sha256sum: {manifest}: {error.strerror}\n'
    for entry in entries:
        match = re.fullmatch(r'([0-9a-fA-F]{64}) [ *](.+)', entry)
        if not match:
            improper += 1
            continue
        proper += 1
        expected, name = match[1].lower(), match[2]
        try:
            actual = digest_file(Path(cwd) / name)
        except OSError as error:
            lines += [f'sha256sum: {name}: {error.strerror}', f'{name}: FAILED open or read']
            unread += 1
            continue
        if actual != expected:
            lines.append(f'{name}: FAILED')
            failed += 1
    if not proper:
        return 1, f'sha256sum: {manifest}: no properly formatted checksum lines found\n'
    for count, one, many in ((improper, 'line is improperly formatted', 'lines are improperly formatted'),
                             (unread, 'listed file could not be read', 'listed files could not be read'),
                             (failed, 'computed checksum did NOT match', 'computed checksums did NOT match')):
        if count:
            lines.append(f'sha256sum: WARNING: {count} {one if count == 1 else many}')
    return int(bool(failed or unread)), ''.join(line + '\n' for line in lines)


# `provision prefix-check`'s status for a check that ran and found the prefix invalid. Any other
# failure (the interpreter, an import, the runtime) means the check did not run.
PREFIX_INVALID = 10


def probe_host(*, env=None, root=ROOT, out=None):
    """Certify the configured overlay install by its utility's actual loader."""
    environment = dict(os.environ if env is None else env)
    try:
        installed = vcpkg.prefix(environment)
        child = vcpkg.environment(environment)
        utility = installed / 'tools/tkrzw/bin/tkrzw_dbm_util'
        if not utility.is_file():
            raise ValueError(f'missing vcpkg utility: {utility}')
        library = runtime.check('tkrzw', 'host check', 'subject', utility, env=child, cwd=root)
        if library is None:
            raise ValueError('the vcpkg utility does not load libtkrzw')
        print(f'host check: {utility} resolves libtkrzw to {library}', file=out or sys.stdout)
        return str(library)
    except (ValueError, runtime.RuntimeRefused) as error:
        print(f'host check: {error}; no healthy host libtkrzw was certified', file=sys.stderr)
        return ''


class Provision:
    """One invocation's paths, qualification state, and provisioning phases."""

    def __init__(self, *, root=ROOT, env=None):
        self.root = Path(root)
        self.env = dict(os.environ if env is None else env)
        self.pin = self.root / 'tools/oracle/oracle-pin.txt'
        # The provisioning cache; the property resolves it on first use, so a run that
        # needs no cache does not need OXPINYIN_CACHE.
        self._cache = None
        self.cells = list(CELLS)
        self.host = platform.system()
        if self.host == 'Linux':
            try:
                self.jobs = text(['nproc'], env=self.env, check=False, stderr=False) or '2'
            except OSError:
                self.jobs = '2'
        else:
            self.jobs = str(os.cpu_count() or 2)
        self.pull, self.python_only, self.dev_tools = True, False, False
        self.platform = ''
        self.started = time.monotonic()
        self.tkrzw_local = False
        self.tkrzw_host_library = ''
        self.ctr = ''
        self.ready = False
        self.inside_image = False
        self.custom_ca = False
        self.net_flags, self.env_flags = ['--network', 'host'], []
        self.uv_dir = str(Path(shutil.which('uv', path=self.env.get('PATH')) or 'uv').parent)

    @property
    def without_oracle(self):
        """--python-only and --dev-tools both stop before the oracle setup."""
        return self.python_only or self.dev_tools

    @property
    def linux(self):
        return self.host == 'Linux'

    @property
    def windows(self):
        """Host Python on Windows cannot read the symlinks a container makes on NTFS: every check
        inside a provisioned prefix runs in the deps image there."""
        return self.host == 'Windows' and not self.inside_image

    @property
    def cache(self):
        if self._cache is None:
            self._cache = images.provision_cache(self.env, self.root)
        return self._cache

    @cache.setter
    def cache(self, value):
        self._cache = Path(value)

    @property
    def oracle_root(self):
        return self.cache / 'oracle'

    @property
    def tkrzw_prefix(self):
        return vcpkg.prefix(self.env)

    @property
    def env_file(self):
        return Path(self.env.get('OXPINYIN_ORACLE_ENV') or str(Path.home() / '.oxpinyin-oracle.env'))

    def log(self, message):
        print(f'[{int(time.monotonic() - self.started):4d}s] {message}', file=sys.stderr, flush=True)

    def die(self, message):
        fail('FAIL: ' + message)

    def field(self, key):
        return images.platform_field(self.pin, self.platform, key)

    def options(self, argv, in_image=False):
        """Preserve setup's options, exclusions, platform and cell checks.

        --cache-dir names the provisioning cache at its mount point IN_IMAGE; a host run takes
        OXPINYIN_CACHE."""
        args, oracle_options = list(argv), False
        while args:
            option = args.pop(0)
            if option in ('-h', '--help'):
                raise Finished(0)  # The retired shell setup printed nothing for --help.
            if option == '--python-only':
                self.python_only = True
            elif option == '--dev-tools':
                self.dev_tools = True
            elif option in ('--cells', '--jobs') or option == '--cache-dir' and in_image:
                oracle_options = True
                if not args:
                    fail(f'{option}: requires an argument', 1)
                value = args.pop(0)
                if option == '--cache-dir':
                    self.cache = value
                elif option == '--cells':
                    self.cells = re.findall(r'[^ \t]+', value.partition('\n')[0])
                else:
                    self.jobs = value
            elif option == '--no-pull':
                oracle_options = True
                self.pull = False
            else:
                fail(f'unknown option: {option}', 2)
        if self.without_oracle and oracle_options:
            fail('--python-only and --dev-tools cannot be combined with --cells, --jobs or --no-pull', 2)
        arch = platform.machine()
        self.platform = {'x86_64': 'linux/amd64', 'AMD64': 'linux/amd64', 'aarch64': 'linux/arm64',
                         'arm64': 'linux/arm64', 'ARM64': 'linux/arm64'}.get(arch, '')
        if not self.platform:
            fail(f'Unsupported oracle architecture {arch}; supported platforms are linux/amd64 and linux/arm64. See docs/runbooks/setup.md.', 2)
        if not self.cells:
            fail('at least one cell is required', 2)
        for cell in self.cells:
            if cell not in CELLS:
                fail(f'unknown cell: {cell} (one of bdb kc tkrzw)', 2)

    def which(self, name):
        return shutil.which(name, path=self.env.get('PATH'))

    def apt(self, packages, sudo=()):
        """Keep the install/update/install retry, including stream suppression."""
        with open(os.devnull, 'wb') as null:
            if not run([*sudo, 'apt-get', 'install', '-y', *packages], env=self.env,
                       stdout=null, stderr=null, check=False):
                return True
            return (not run([*sudo, 'apt-get', 'update'], env=self.env, stdout=null, stderr=null, check=False)
                    and not run([*sudo, 'apt-get', 'install', '-y', *packages], env=self.env, stdout=null, check=False))

    def prerequisites(self):
        """Require native tools only for full setup; share doctor's version formats."""
        if self.without_oracle:
            return
        for name, diagnostic in (
                ('cargo', 'cargo is missing; install the rustup-managed toolchain from rust-toolchain.toml (docs/runbooks/setup.md)'),
                ('rustup', 'rustup is missing; install the repository toolchain via docs/runbooks/setup.md')):
            if not self.which(name):
                self.die(diagnostic)
        match = re.search(r'^channel = "([^"]*)"', (self.root / 'rust-toolchain.toml').read_text(), re.M)
        if not match:
            self.die('no channel in rust-toolchain.toml')
        channel = match[1]
        with open(os.devnull, 'wb') as null:
            for name in ('rustc', 'cargo', 'rustfmt', 'clippy-driver'):
                if run(['rustup', 'run', channel, name, '--version'], env=self.env, stdout=null, stderr=null, check=False):
                    self.die(f'Rust toolchain {channel} is not installed; run: rustup toolchain install {channel} --component clippy --component rustfmt')
        if not self.linux:
            # Valgrind, its apt install and the root check are Linux only; no container step needs it.
            self.valgrind_version = 'not checked (Linux only)'
            return
        tool = next(tool for tool in doctor.TOOLS if tool.name == 'valgrind')
        def found():
            return text(['valgrind', '--version'], env=self.env, check=False, stderr=False) if self.which('valgrind') else ''
        def acceptable(value):
            parsed = tool.format.parse(value)
            return parsed is not None and parsed.key >= tool.format.key(tool.minimum)
        value = found()
        if not acceptable(value):
            state = f'{value} is older than {tool.minimum}' if value else 'valgrind is missing'
            note = f'outdated: {value} (minimum {tool.minimum})' if value else 'missing: valgrind'
            sudo = []
            if os.geteuid() != 0:
                if not self.which('sudo'):
                    print(f'{note}\nrun as root: apt-get install -y valgrind', file=sys.stderr)
                    self.die(f'{state} and neither root nor sudo is available')
                sudo = ['sudo']
            if not self.which('apt-get'):
                self.die(f'{state} and apt-get is unavailable; ask the environment owner to install Valgrind {tool.minimum} or newer')
            self.log('installing valgrind from the distribution archive')
            if not self.apt(['valgrind'], sudo):
                self.die('could not install valgrind with apt-get')
            if not acceptable(found()):
                self.die(f'valgrind {tool.minimum} or newer required; the distribution archive provides: {found() or "none"}')
        self.valgrind_version = found()

    def accepted(self, tool):
        """Whether TOOL is on PATH and meets its requirement, by the doctor's own check."""
        if not self.which(tool.executable):
            return False
        try:
            doctor.check_tool(tool, self.env)
        except doctor.DoctorError:
            return False
        return True

    def cargo_tools(self):
        """Keep tools at their floors or install their pinned crates.io fallbacks."""
        for crate, pin in doctor.CARGO_TOOL_PINS.items():
            tool = doctor.cargo_tool(crate, self.root)
            if self.accepted(tool):
                continue
            if not self.which('cargo'):
                self.die(f'installing {crate} needs cargo; follow docs/runbooks/setup.md')
            self.log(f'installing {crate} {pin} with cargo (requires {doctor.requirement(tool)})')
            if run([self.which('cargo'), 'install', crate, '--version', pin, '--locked', '--quiet'], env=self.env, cwd=self.root, check=False):
                self.die(f'cargo install {crate} failed; allow index.crates.io and static.crates.io '
                         '(docs/runbooks/cloud.md)')
            if not self.accepted(tool):
                self.die(f'{crate} does not meet {doctor.requirement(tool)} after cargo install; '
                         'put $CARGO_HOME/bin on PATH')
            self.log('installed ' + doctor.check_tool(tool, self.env))
        # The setup entry does not start from a just recipe, so it asks just for the cache root
        # that the recipes export (docs/runbooks/setup.md#cache-directory). Without an answer the
        # variable stays unset: a run that needs the cache refuses, and --python-only needs none.
        if 'OXPINYIN_CACHE' not in self.env:
            done = capture([self.which('just') or 'just', '--justfile', str(self.root / 'justfile'), 'cache-root'],
                           env=self.env, check=False, stderr=False)
            if not done.status:
                # The Windows recipe ends its line with CRLF.
                self.env['OXPINYIN_CACHE'] = done.stdout.decode().rstrip('\r\n')

    def rust_analyzer(self):
        """Keep the toolchain's rust-analyzer component, or add it with rustup.

        Development hosts need it; CI does not, so rust-toolchain.toml does not list it. rustup
        runs in the checkout, so the component belongs to the toolchain that file selects."""
        # Each program runs at the path found on self.env's PATH: on Windows a bare name
        # resolves against the PATH of this process instead.
        rustup, analyzer = self.which('rustup'), self.which('rust-analyzer')
        with open(os.devnull, 'wb') as null:
            if not rustup:
                # Without rustup no proxy answers for a missing component: a rust-analyzer
                # that runs is a real one, and it is kept.
                if analyzer and not run([analyzer, '--version'], env=self.env, cwd=self.root,
                                        stdout=null, stderr=null, check=False):
                    return
                self.die('adding rust-analyzer needs rustup; follow docs/runbooks/setup.md')
            if not run([rustup, 'which', 'rust-analyzer'], env=self.env, cwd=self.root,
                       stdout=null, stderr=null, check=False):
                return
            self.log('adding the rust-analyzer component with rustup')
            if run([rustup, 'component', 'add', 'rust-analyzer'], env=self.env, cwd=self.root,
                   stdout=null, check=False):
                self.die('rustup component add rust-analyzer failed')
            if run([rustup, 'which', 'rust-analyzer'], env=self.env, cwd=self.root,
                   stdout=null, stderr=null, check=False):
                self.die('rust-analyzer is missing after rustup component add')
        self.log('added ' + text([self.which('rust-analyzer') or 'rust-analyzer', '--version'], env=self.env, cwd=self.root))

    def report_ext4(self):
        """Report writable candidates without claiming measurement readiness."""
        if not self.linux:
            print('ext4 paths: not checked (findmnt and the measurement gates are Linux only).')
            return
        if not self.which('findmnt'):
            print('ext4 paths: unknown (findmnt unavailable); measurement gates require a verified ext4 path.')
            return
        done = capture(['findmnt', '--raw', '--noheadings', '--output', 'TARGET,FSTYPE'], env=self.env, check=False, stderr=False)
        unknown, found = bool(done.status), False
        candidates = [str(self.root), self.env.get('HOME', str(Path.home())), self.env.get('TMPDIR') or '/tmp',
                      *([str(self.cache)] if self._cache or self.env.get('OXPINYIN_CACHE') else [])]
        for line in done.stdout.decode().splitlines():
            parts = line.split()
            if len(parts) == 2 and parts[1] == 'ext4':
                candidates.append(re.sub(r'\\x([0-9a-fA-F]{2})', lambda m: chr(int(m[1], 16)), parts[0]))
        for path in dict.fromkeys(candidates):
            if not Path(path).is_dir() or not os.access(path, os.R_OK | os.W_OK | os.X_OK):
                continue
            done = capture(['findmnt', '--noheadings', '--output', 'FSTYPE', '--target', path], env=self.env, check=False, stderr=False)
            unknown |= bool(done.status)
            if not done.status and done.stdout.decode().strip() == 'ext4':
                print(f'ext4 path (readable, writable, searchable): {path}')
                found = True
        if found:
            print('Measurement paths still need individual checks with findmnt -T <path>: libraries, data, profiles, work directory and TMPDIR.')
        elif unknown:
            print('ext4 paths: unknown (mount detection failed); measurement gates require a verified ext4 path.')
        else:
            print('ext4 paths: none accessible among mount targets, checkout, HOME, TMPDIR and cache; measurement gates cannot run without an ext4 path.')
        print('This report does not verify all measured files are on ext4; non-measurement setup may continue.')

    def oracle(self, argv):
        """Provision only oracle cells inside the native oracle performance image."""
        self.refuse_overrides()
        self.options(argv, in_image=True)
        if self.without_oracle or not str(self.jobs).isdecimal() or int(self.jobs) < 1:
            fail('provision oracle needs positive --jobs and does not accept --python-only or --dev-tools', 2)
        marker = Path('/usr/local/share/oxpinyin/performance-os-release')
        if (self.env.get('GITHUB_ACTIONS') == 'true' or not marker.is_file()
                or marker.read_bytes() != Path('/etc/os-release').read_bytes()):
            self.die('provision oracle runs locally inside just provision-oracle (or just performance)')
        self.cache = self.cache.resolve()
        self.cache.mkdir(parents=True, exist_ok=True)
        self.model_cache = images.model_cache(self.env)
        self.inside_image, self.pull = True, False
        self.oracle_root.mkdir(parents=True, exist_ok=True)
        self.libraries()
        for backend in self.cells:
            prefix = self.oracle_root / backend
            common = [sys.executable, self.root / 'tools/oracle/build.py', '--dbm', backend,
                      '--prefix', prefix, '--enable-libzhuyin', '--jobs', self.jobs]
            if not self.data_ok(backend):
                extracted = model.fetch(self.model_cache, root=self.root, env=self.env)
                run([*common, '--regen-data', '--model-dir', extracted,
                     '--work-dir', self.cache / ('data-' + backend)], env=self.env, heavy=True)
            if not self.data_ok(backend):
                self.die(f'{backend}: generated data failed provenance validation')
            self.log(f'{backend}: current pin/recipe, recorded library hashes and data manifests OK')
        return 0

    def libs_ok(self, cell, prefix=None):
        prefix = Path(prefix) if prefix is not None else self.oracle_root / cell
        if self.windows:
            return self.contained_ok('libs', cell, prefix)
        return images.prefix_ok(prefix, cell, self.root, self.pin, self.platform)

    def contained(self, check, cell, prefix):
        """Run one `prefix-check CHECK` of PREFIX in the deps image; answer its status."""
        self.contained_ready()
        prefix = Path(prefix)
        return run(runtime_command([
            self.ctr, 'run', '--rm', '--platform', self.platform, '--network', 'none',
            '-e', 'PYTHONDONTWRITEBYTECODE=1', '-v', f'{self.root}:/repo:ro',
            '-v', f'{prefix.parent}:/opt/pinyin-oracle:ro', '-v', f'{self.pin}:/run/oracle-pin.txt:ro',
            '-w', '/repo', self.deps_image, 'uv', 'run', '--no-project', '--managed-python', 'python',
            '-X', 'utf8', '-m', 'tools.dev.provision', 'prefix-check', check, cell,
            f'/opt/pinyin-oracle/{prefix.name}', '/run/oracle-pin.txt', *([str(prefix)] if check == 'summary' else [])]),
            env=self.env, check=False)

    def contained_ok(self, check, cell, prefix):
        """Whether the contained CHECK found PREFIX valid; a check that did not run stops provisioning.

        Only a check that ran may lead to a removal: any status but 0 and PREFIX_INVALID dies
        here, naming it, and the prefix is kept."""
        status = self.contained(check, cell, prefix)
        if status not in (0, PREFIX_INVALID):
            self.die(f'{cell}: the {check} check of {prefix} did not run ({self.ctr} exited {status}); the prefix is kept')
        return not status

    def contained_ready(self):
        """The runtime answers and the deps image is present, for the checks run inside it."""
        self.runtime_ready()
        if not getattr(self, 'deps_present', False):
            self.build_deps_image()
            self.deps_present = True

    def in_deps_image(self, script, *args):
        """Run a sh SCRIPT in the deps image with the cache at /cache (Windows prefix changes)."""
        self.contained_ready()
        run(runtime_command([self.ctr, 'run', '--rm', '--platform', self.platform, '--network', 'none',
                             '-v', f'{self.cache}:/cache', self.deps_image, 'sh', '-c', script, 'sh', *args]),
            env=self.env, stdout=subprocess.DEVNULL)

    def remove_prefix(self, cell):
        if self.windows:
            self.in_deps_image('rm -rf "/cache/oracle/$1"', cell)
        else:
            shutil.rmtree(self.oracle_root / cell, ignore_errors=True)

    def summary(self, cell, prefix, shown=None):
        return (f'{cell} prefix={shown or prefix} libpinyin={digest_file(prefix / "lib/libpinyin.so")} '
                f'libzhuyin={digest_file(prefix / "lib/libzhuyin.so")} data manifest OK')

    def data_ok(self, cell, prefix=None):
        """Check stable pin identity and prefix-local generated manifest provenance."""
        prefix = Path(prefix) if prefix is not None else self.oracle_root / cell
        if self.windows:
            return self.contained_ok('data', cell, prefix)
        if not self.libs_ok(cell, prefix):
            return False
        if not images.model_ok(prefix, self.root, self.pin):
            return False
        record = images.fields(prefix / 'oracle-pin.txt')
        stable = prefix / 'oracle-data.sha256'
        if not stable.is_file() or digest_file(stable) != record.get('data_manifest_sha256'):
            return False
        if check_manifest(stable.name, prefix)[0]:
            return False
        unstable = prefix / 'oracle-data-unstable.sha256'
        provenance = prefix / 'oracle-pin.txt'
        if not unstable.is_file():
            print(f'data manifest missing: {unstable}', file=sys.stderr)
            return False
        if not provenance.is_file():
            print(f'data manifest provenance missing: {provenance}', file=sys.stderr)
            return False
        expected = images.fields(provenance).get('data_unstable_manifest_sha256', '')
        if not expected or digest_file(unstable) != expected:
            print(f'data manifest SHA-256 differs from {provenance}: {unstable}', file=sys.stderr)
            return False
        status, output = check_manifest(unstable.name, prefix)
        sys.stderr.write(output)
        return not status

    def write_oracle_env(self):
        """Retain only verified cells and atomically publish one sourceable file."""
        path = self.env_file
        env_cells = list(self.cells)
        prefixes = {cell: str(self.oracle_root / cell) for cell in env_cells}
        previous = read_env(path)
        if path.is_file() and not previous:
            self.log(f'{path}: not written by this provisioner; regenerated with the cells of this run')
        kept_prefix, kept_local, kept_library = '', False, ''
        for cell in CELLS:
            prefix = previous.get(f'OXPINYIN_ORACLE_PREFIX_{cell.upper()}', '')
            if cell in prefixes or not prefix:
                continue
            if not self.libs_ok(cell, prefix):
                self.log(f'{cell}: removed from environment file; current pin/recipe or recorded library bytes failed validation')
            elif not self.data_ok(cell, prefix):
                self.log(f'{cell}: removed from environment file; data manifest check failed')
            else:
                prefixes[cell] = prefix
                env_cells.append(cell)
                if cell == 'tkrzw':
                    kept_prefix = previous.get('OXPINYIN_TKRZW_PREFIX', '')
                    kept_local = bool(kept_prefix) and 'PKG_CONFIG_PATH' in previous
                    kept_library = previous.get('OXPINYIN_TKRZW_LIBRARY', '')
        if len(env_cells) == 1:
            default = env_cells[0]
        else:
            preferred = backend_cell.default_cell()
            default = preferred if preferred in env_cells else env_cells[0]
        runtime_prefix = str(self.tkrzw_prefix) if self.tkrzw_local else ''
        local_prefix = runtime_prefix
        library = self.tkrzw_host_library
        if not runtime_prefix and not library and kept_local:
            runtime_prefix = local_prefix = kept_prefix
        if runtime_prefix:
            obj = Path(runtime_prefix) / 'lib/libtkrzw.so.1'
            library = str(obj.resolve()) if obj.exists() else ''
        elif not library:
            library = kept_library
        lines = [ENV_HEADER,
                 'export PINYIN_MODEL_DIR=' + quote(self.model_cache / 'extracted'),
                 'export PATH=' + quote(self.uv_dir) + ':"$PATH"']
        if library:
            lines.append('export OXPINYIN_TKRZW_LIBRARY=' + quote(library))
        if runtime_prefix:
            lines += ['export OXPINYIN_TKRZW_PREFIX=' + quote(runtime_prefix),
                      'export OXPINYIN_TKRZW_PREFIXES=' + quote(runtime_prefix) + '"${OXPINYIN_TKRZW_PREFIXES:+:$OXPINYIN_TKRZW_PREFIXES}"']
        if local_prefix:
            lines += ['export PKG_CONFIG_PATH=' + quote(local_prefix + '/lib/pkgconfig') + '"${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"',
                      'export LD_LIBRARY_PATH=' + quote(local_prefix + '/lib') + '"${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"',
                      'export PATH=' + quote(local_prefix + '/tools/tkrzw/bin') + ':"$PATH"']
        elif library:
            lines.append('export LD_LIBRARY_PATH=' + quote(Path(library).parent) + '"${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"')
        for cell in env_cells:
            lines += [f'export OXPINYIN_ORACLE_PREFIX_{cell.upper()}=' + quote(prefixes[cell]),
                      f'export OXPINYIN_ORACLE_DATA_{cell.upper()}=' + quote(prefixes[cell] + '/lib/libpinyin/data')]
        # One arm per provisioned cell, its values computed here; no shell expansion selects them.
        lines += ['export PINYIN_ORACLE_DBM="${PINYIN_ORACLE_DBM:-' + default + '}"',
                  'case "$PINYIN_ORACLE_DBM" in']
        lines += [f'{cell}) export PINYIN_ORACLE_PREFIX={quote(prefixes[cell])} '
                  f'OXPINYIN_LIBPINYIN_DATA_DIR={quote(prefixes[cell] + "/lib/libpinyin/data")} ;;'
                  for cell in env_cells]
        lines += ["*) printf '%s\\n' \"cell $PINYIN_ORACLE_DBM was not provisioned by this environment file\" >&2; return 1 ;;",
                  'esac']
        staging = Path(str(path) + '.new')
        try:
            staging.write_text('\n'.join(lines) + '\n')
            # Preserve the external mv's launch/failure statuses and atomic rename.
            run(['mv', str(staging), str(path)], env=self.env)
        finally:
            staging.unlink(missing_ok=True)
        self.log(f'wrote {path}; source it before a runner')

    def proxy_ca(self, stack):
        """Choose existing supported trust and mount only a private 0600 copy."""
        ccr = Path(self.env.get('OXPINYIN_CCR_DIR') or '/root/.ccr')
        self.proxy = self.env.get('HTTPS_PROXY') or self.env.get('https_proxy', '')
        self.ca = Path(self.env.get('OXPINYIN_PROXY_CA') or str(ccr / 'ca-bundle.crt'))
        self.custom_ca = False
        def present(path):
            return path.is_file() and path.stat().st_size > 0
        if self.env.get('OXPINYIN_PROXY_CA'):
            if not present(self.ca):
                self.die(f'OXPINYIN_PROXY_CA bundle is missing: {self.ca}')
            self.custom_ca = True
            self.log(f'proxy CA: {self.ca} (OXPINYIN_PROXY_CA)')
        elif present(self.ca):
            self.custom_ca = True
            self.log(f'proxy CA: {self.ca} (session bundle)')
        elif self.proxy:
            candidates = [('SSL_CERT_FILE', self.env.get('SSL_CERT_FILE', '')), ('CURL_CA_BUNDLE', self.env.get('CURL_CA_BUNDLE', ''))]
            candidates += [('system bundle', p) for p in self.env.get('OXPINYIN_SYSTEM_CA_BUNDLES', '/etc/ssl/certs/ca-certificates.crt:/etc/pki/tls/certs/ca-bundle.crt:/etc/ssl/cert.pem').split(':')]
            for label, path in candidates:
                if path and present(Path(path)):
                    self.ca, self.custom_ca = Path(path), True
                    self.log(f'proxy CA: HTTPS_PROXY is set and {ccr}/ca-bundle.crt is absent; using the host-trusted {label}: {self.ca}')
                    self.log("proxy CA: a present, non-empty bundle does not show that the proxy's interception certificate is trusted; if TLS then fails, set OXPINYIN_PROXY_CA to the proxy's CA bundle")
                    break
            if not self.custom_ca:
                self.die(f"HTTPS_PROXY is set but there is no proxy CA bundle: {ccr}/ca-bundle.crt is absent and the host trusts none (SSL_CERT_FILE, CURL_CA_BUNDLE, system bundle); set OXPINYIN_PROXY_CA to the proxy's CA bundle")
        self.ca_mount = self.ca
        if self.custom_ca:
            temporary = Path(stack.enter_context(tempfile.TemporaryDirectory()))
            self.ca_mount = temporary / 'ca.crt'
            try:
                with self.ca_mount.open('xb') as output:
                    self.ca_mount.chmod(0o600)
                    with self.ca.open('rb') as source:
                        shutil.copyfileobj(source, output)
            except OSError:
                self.die(f'could not copy the proxy CA bundle: {self.ca}')
            self.log(f'proxy CA: containers mount a private copy at {self.ca_mount} (mode 0600, removed on exit)')
        if self.proxy:
            no_proxy = self.env.get('NO_PROXY') or self.env.get('no_proxy', '')
            self.env_flags = ['-e', f'https_proxy={self.proxy}', '-e', f'HTTPS_PROXY={self.proxy}',
                              '-e', f'no_proxy={no_proxy}', '-e', f'NO_PROXY={no_proxy}']
            if self.custom_ca:
                self.env_flags += ['-e', 'GIT_SSL_CAINFO=/run/ca.crt', '-e', 'SSL_CERT_FILE=/run/ca.crt',
                                   '-e', 'CURL_CA_BUNDLE=/run/ca.crt', '-v', f'{self.ca_mount}:/run/ca.crt:ro,z']

    def retry(self, command):
        for delay in (2, 4, 8, 16):
            if not run(command, env=self.env, check=False):
                return
            self.log(f'retrying in {delay}s: {" ".join(map(str, command))}')
            if self.linux:
                run(['sleep', str(delay)], env=self.env)
            else:
                time.sleep(delay)
        run(command, env=self.env)

    def backend_tools(self):
        missing = [(tool, package) for cell, tool, package in
                   (('bdb', 'db5.3_dump', 'db5.3-util'), ('kc', 'kchashmgr', 'kyotocabinet-utils'))
                   if cell in self.cells and not self.which(tool)]
        if missing and not self.linux:
            # Provisioning runs neither tool; off Linux every prefix check runs in the deps image.
            self.log(f'{" ".join(tool for tool, _ in missing)}: not required on {self.host}; '
                     'provisioning checks run in the deps image there')
            return
        if missing:
            tools, packages = map(list, zip(*missing))
            if os.geteuid() == 0:
                self.log(f'installing {" ".join(packages)} from the distribution archive')
                if not self.apt(packages):
                    self.die(f'could not install {" ".join(packages)} with apt-get')
            else:
                fail(f'missing: {" ".join(tools)}\nrun as root: apt-get install -y {" ".join(packages)}')

    def runtime_ready(self):
        if self.ready:
            return
        self.ready = True
        def ready():
            with open(os.devnull, 'wb') as null:
                return not run(runtime_command([self.ctr, 'info']), env=self.env, stdout=null, stderr=null, check=False)
        if ready():
            self.log(f'{self.ctr} ready')
        elif Path(self.ctr).name == 'docker' and not self.linux:
            self.die('docker info failed; start Docker Desktop or the Docker daemon (dockerd is started only on Linux)')
        elif Path(self.ctr).name == 'docker':
            if not self.which('dockerd'):
                self.die('dockerd is not installed')
            single = Path(self.env.get('OXPINYIN_CCR_DIR') or '/root/.ccr') / 'agent-proxy-ca.crt'
            destination = Path('/usr/local/share/ca-certificates/ccr-agent-proxy.crt')
            if self.proxy and single.is_file() and single.stat().st_size and (not destination.is_file() or single.read_bytes() != destination.read_bytes()):
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(single, destination)
                with open(os.devnull, 'wb') as null:
                    if run(['update-ca-certificates'], env=self.env, stdout=null, stderr=null, check=False):
                        self.log('update-ca-certificates failed; the daemon may not trust the proxy')
            self.log('starting dockerd')
            with open('/var/log/dockerd.log', 'wb') as output, open(os.devnull, 'rb') as input_file:
                subprocess.Popen(['dockerd'], env=self.env, stdin=input_file, stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
            for _ in range(60):
                if ready():
                    break
                run(['sleep', '1'], env=self.env)
            if not ready():
                self.tail('/var/log/dockerd.log', 20)
                self.die('dockerd did not become ready in 60 s')
        else:
            run(runtime_command([self.ctr, 'info']), env=self.env, stdout=sys.stderr, check=False)
            self.die(f'{self.ctr} info failed; there is no daemon to start for it')

    def tail(self, path, count=5, logged=False):
        for line in Path(path).read_text(errors='replace').splitlines()[-count:]:
            self.log('  ' + line) if logged else print(line, file=sys.stderr)

    def build_deps_image(self):
        if self.inside_image:
            return
        with open(os.devnull, 'wb') as null:
            if not run(runtime_command([self.ctr, 'image', 'inspect', self.deps_image]), env=self.env, stdout=null, stderr=null, check=False):
                return
            if self.pull:
                with (self.cache / 'pull-deps.err').open('wb') as error:
                    status = run(runtime_command([self.ctr, 'pull', '--quiet', '--platform', self.platform, self.published_deps]), env=self.env, stdout=null, stderr=error, check=False)
                if not status:
                    run(runtime_command([self.ctr, 'tag', self.published_deps, self.deps_image]), env=self.env)
                    return
                self.log(f'recorded deps image unavailable; building locally (details: {self.cache}/pull-deps.err)')
                self.tail(self.cache / 'pull-deps.err')
            if run(runtime_command([self.ctr, 'image', 'inspect', self.base_image]), env=self.env, stdout=null, stderr=null, check=False):
                self.log(f'pulling {self.base_image} for the local fallback build')
                self.retry(runtime_command([self.ctr, 'pull', '--quiet', '--platform', self.platform, self.base_image]))
        self.log(f'building {self.deps_image} (the testing pin-reference toolchain)')
        flags = list(self.net_flags)
        if self.platform != 'linux/amd64':
            flags += ['--build-arg', f'ORACLE_PLATFORM={self.platform}']
        if self.proxy:
            flags += ['--build-arg', 'https_proxy', '--build-arg', 'HTTPS_PROXY']
            if self.custom_ca:
                flags += ['--secret', f'id=cacert,src={self.ca}']
        run(runtime_command([self.ctr, 'build', *images.performance_resources(self.ctr), '--platform', self.platform, *flags, '--target', 'deps',
                             '-f', self.root / 'tools/oracle/Dockerfile.oracle', '-t', self.deps_image, self.root]),
            env=dict(self.env, DOCKER_BUILDKIT='1', https_proxy=self.proxy, HTTPS_PROXY=self.proxy), stdout=2, heavy=True)

    def in_image_command(self, args, flags=()):
        """The container command that runs the checkout's tools/oracle/build.py with ARGS.

        The checkout is mounted over the image's /opt/oracle-recipe, so the builder, its pin
        record and the recipe hash are the checkout's; Debian's python3 runs it."""
        apple = images.is_apple(self.ctr)
        return runtime_command([
            self.ctr, 'run', '--rm', '--platform', self.platform, *(['--memory', '6G'] if apple else []),
            *self.net_flags, *self.env_flags, *flags,
            '-v', f'{self.root}:/opt/oracle-recipe:ro,z',
            '-v', f'{self.oracle_root}:/opt/pinyin-oracle:z', '-v', f'{self.model_cache}:/model-cache:ro,z',
            self.deps_image, 'python3', '/opt/oracle-recipe/tools/oracle/build.py', *args])

    def pool_phase(self, label, cells, log, args):
        """Reuse #1233's exact pool phase and owned-container cancellation."""
        if self.inside_image:
            mappings = {'/opt/pinyin-oracle': self.oracle_root, '/build': self.cache / 'build',
                        '/model-cache': self.model_cache}
            for backend in cells:
                arguments = []
                for value in args:
                    value = str(value).format(item=backend, jobs=self.jobs)
                    for original, replacement in mappings.items():
                        if value == original or value.startswith(original + '/'):
                            value = str(replacement) + value[len(original):]
                            break
                    arguments.append(value)
                self.log(f'{backend}: {label} with current pin/recipe')
                with Path(str(log).format(item=backend)).open('wb') as output:
                    status = run([sys.executable, self.root / 'tools/oracle/build.py', *arguments],
                                 env=self.env, stdout=output, heavy=True, check=False)
                if status:
                    return status
            return 0
        flags = ['--name', f'oxpinyin-setup-{{token}}-{label}-{{item}}', '--label', 'oxpinyin.setup={token}',
                 '--cidfile', '{cidfile}', '--cpus', '{jobs}']
        owner = ['--container-runtime', shlex.quote(self.ctr), '--container-label', 'oxpinyin.setup']
        # A subprocess preserves the invocation's environment without mutating
        # Python's process-global environ during pool scheduling.
        return run([sys.executable, '-X', 'utf8', '-m', 'tools.dev.pool', 'run',
                    '--label', label, '--items', ','.join(cells), '--cpus', str(self.jobs),
                    '--log', str(log), *owner, '--', *self.in_image_command(args, flags)],
                   cwd=self.root, env=self.env, check=False)

    def libraries(self):
        need = []
        for cell in self.cells:
            if self.libs_ok(cell):
                self.log(f'{cell}: libraries already match current pin/recipe and recorded bytes')
            else:
                need.append(cell)
        if need and self.pull and images.is_apple(self.ctr):
            self.log('Apple container: building the libraries in the deps image (no published-image pull)')
        elif need and self.pull:
            self.log(f'trying {self.published}')
            error_path = self.cache / 'pull-published.err'
            with error_path.open('wb') as error, open(os.devnull, 'wb') as null:
                status = run(runtime_command([self.ctr, 'pull', '--quiet', '--platform', self.platform, self.published]), env=self.env, stdout=null, stderr=error, check=False)
            if not status:
                cid = text(runtime_command([self.ctr, 'create', '--platform', self.platform, self.published, '/none']),
                           env=self.env).rstrip('\r')  # wslc ends its lines with CRLF
                try:
                    self.copy_published(cid, need)
                finally:
                    run(runtime_command([self.ctr, 'rm', cid]), env=self.env, stdout=subprocess.DEVNULL, check=False)
                still = []
                for cell in need:
                    if self.libs_ok(cell):
                        self.log(f'{cell}: pulled libraries match current pin and recipe')
                    else:
                        self.log(f'{cell}: pulled libraries have stale or invalid build provenance; building instead')
                        self.remove_prefix(cell)
                        still.append(cell)
                need = still
            else:
                if re.search(r'manifest unknown|manifest .*not found|name unknown|404 not found|: not found$', error_path.read_text(), re.I | re.M):
                    self.log('published oracle image: manifest unknown / not found; building the libraries')
                else:
                    self.log('published oracle image: pull FAILED (network, auth or proxy, not "not found"); building the libraries')
                self.log(f'pull error (last lines, full text in {error_path}):')
                self.tail(error_path, logged=True)
        if need:
            self.build_deps_image()
            for cell in need:
                self.remove_prefix(cell)
            if self.pool_phase('libraries', need, self.cache / 'build-{item}.log',
                               ['--libs-only', '--enable-libzhuyin', '--dbm', '{item}', '--work-dir', '/build/{item}',
                                '--prefix', '/opt/pinyin-oracle/{item}', '--jobs', '{jobs}']):
                self.die(f'the library build failed or was cancelled (logs: {self.cache}/build-<cell>.log)')
        for cell in self.cells:
            if not self.libs_ok(cell):
                self.die(f'{cell}: stale pin/recipe or damaged build provenance after provisioning')

    def copy_published(self, cid, cells):
        """Copy CELLS' prefixes out of the published image's container CID into the cache.

        On Windows the filesystem is exported to a tar in the cache and unpacked by the deps image,
        so the prefix's symlinks are made by Linux and never read by host Python."""
        if not self.windows:
            for cell in cells:
                shutil.rmtree(self.oracle_root / cell, ignore_errors=True)
                run(runtime_command([self.ctr, 'cp', f'{cid}:/opt/pinyin-oracle/{cell}', self.oracle_root / cell]),
                    env=self.env, stdout=subprocess.DEVNULL)
            return
        archive = self.cache / 'published.tar'
        try:
            run(runtime_command([self.ctr, 'export', '-o', archive, cid]), env=self.env, stdout=subprocess.DEVNULL)
            self.in_deps_image('set -eu; scratch=$(mktemp -d); tar -xf /cache/published.tar -C "$scratch"; '
                               'for cell; do rm -rf "/cache/oracle/$cell"; '
                               'cp -a "$scratch/opt/pinyin-oracle/$cell" "/cache/oracle/$cell"; done', *cells)
        finally:
            archive.unlink(missing_ok=True)

    def sound_tkrzw(self):
        if 'tkrzw' not in self.cells:
            return
        try:
            vcpkg.install(self.env, self.root, jobs=self.jobs)
            prefix = self.tkrzw_prefix
            child = vcpkg.environment(self.env)
            utility = prefix / 'tools/tkrzw/bin/tkrzw_dbm_util'
            for side, binary in (('subject', utility), ('pin', self.oracle_root / 'tkrzw/lib/libpinyin.so')):
                if runtime.check('tkrzw', 'provision', side, binary, env=child, cwd=self.root) is None:
                    raise ValueError(f'{side} binary does not load libtkrzw: {binary}')
        except (ValueError, runtime.RuntimeRefused) as error:
            self.die(str(error))
        self.tkrzw_local = True
        self.log(f'tkrzw: certified vcpkg install at {prefix}')

    def refuse_overrides(self):
        """Refuse interpreter-selection settings before any other work.

        uv has already read them for `uv python install` and `uv run`, so a run under one
        stops here rather than provisioning with another interpreter."""
        try:
            doctor.reject_overrides(self.env, before='provisioning')
        except doctor.DoctorError as error:
            self.die(str(error))

    def execute(self, argv):
        self.refuse_overrides()
        self.options(argv)
        self.prerequisites()
        # --python-only alone needs no Cargo, rustup or just (docs/runbooks/cloud.md).
        if self.dev_tools or not self.python_only:
            self.cargo_tools()
            self.rust_analyzer()
        self.log('using ' + text(['uv', '--version'], env=self.env))
        # uv selected the managed interpreter. Doctor owns its policy; provisioning
        # keeps the successful verification line.
        identity = doctor.identity(self.env)
        print('Verified managed CPython', identity['version'], 'GIL build:', identity['executable'], flush=True)
        if self.without_oracle:
            self.report_ext4()
            self.log('stopping after the development tools (--dev-tools)' if self.dev_tools
                     else 'stopping after Python provisioning (--python-only)')
            return 0
        with ExitStack() as stack:
            self.proxy_ca(stack)
            self.cache.mkdir(parents=True, exist_ok=True)
            self.cache = self.cache.resolve()
            self.model_cache = images.model_cache(self.env)
            self.model_cache.mkdir(parents=True, exist_ok=True)
            self.model_cache = self.model_cache.resolve()
            self.oracle_root.mkdir(parents=True, exist_ok=True)
            self.base_image = images.fields(self.pin).get('base_image', '')
            if not self.base_image:
                self.die(f'no base_image in {self.pin}')
            published = 'ghcr.io/shenghaoc/oxpinyin-oracle:' + images.image_tag(self.root)
            self.published = published + ('-arm64' if self.platform == 'linux/arm64' else '')
            self.published_deps = self.published + '-deps'
            digest = hashlib.sha256()
            # .python-version: the deps stage installs the managed interpreter it names.
            for path in ('tools/oracle/Dockerfile.oracle', 'tools/oracle/versions.py', 'tools/oracle/version-manifest.json', 'tools/oracle/oracle-pin.txt', '.python-version'):
                digest.update((self.root / path).read_bytes())
            self.deps_image = 'oxpinyin-oracle-deps:' + digest.hexdigest()[:12]
            self.ctr = images.select_runtime(env=self.env)
            self.backend_tools()
            for cell in self.cells:
                if not self.libs_ok(cell) or not self.data_ok(cell):
                    try:
                        model.fetch(self.model_cache, root=self.root, env=self.env)
                    except Finished:
                        self.die('model20 is unavailable; allow downloads.sourceforge.net and its *.dl.sourceforge.net redirect target, or supply a verified local model cache (docs/runbooks/cloud.md)')
                    break
            self.runtime_ready()
            self.libraries()
            if self.linux:
                self.sound_tkrzw()
            else:
                self.log(f'tkrzw: no host libtkrzw check on {self.host}; the oracle runs only in containers here')
            need = []
            for cell in self.cells:
                if self.data_ok(cell):
                    self.log(f'{cell}: data already matches the recorded manifest')
                else:
                    need.append(cell)
            if need:
                self.log(f'fetching model20 for {" ".join(need)}')
                model.fetch(self.model_cache, root=self.root, env=self.env)
                self.build_deps_image()
                if self.pool_phase('data', need, self.cache / 'data-{item}.log',
                                   ['--regen-data', '--dbm', '{item}', '--prefix', '/opt/pinyin-oracle/{item}',
                                    '--model-dir', '/model-cache/extracted', '--work-dir', '/build/{item}-data', '--jobs', '{jobs}']):
                    self.die(f'--regen-data failed or was cancelled (logs: {self.cache}/data-<cell>.log)')
                for cell in need:
                    if not self.data_ok(cell):
                        self.die(f'{cell}: the regenerated data does not match the recorded manifest')
            self.report_ext4()
            print(f'Valgrind: {self.valgrind_version}')
            for cell in self.cells:
                prefix = self.oracle_root / cell
                if self.windows:
                    if self.contained('summary', cell, prefix):
                        self.die(f'{cell}: the summary check in the deps image failed')
                    continue
                if self.linux:
                    for pc in (prefix / 'lib/pkgconfig').glob('*.pc'):
                        pc.write_text(re.sub(r'^prefix=.*$', lambda _: 'prefix=' + str(prefix), pc.read_text(), flags=re.M))
                print(self.summary(cell, prefix), flush=True)
            if self.linux:
                self.write_oracle_env()
            else:
                self.log(f'{self.env_file}: not written; host runners load the oracle only on Linux')
        return 0


def main(argv=None):
    """Dispatch cloud provisioning, including native host callers."""
    args = list(sys.argv[1:] if argv is None else argv)
    service = None
    try:
        with slot.cancellation_handlers():
            if args[:1] == ['oracle']:
                service = Provision()
                return service.oracle(args[1:])
            if args[:1] == ['probe-host']:
                library = probe_host()
                if not library:
                    return 1
                print(library)
                return 0
            if args[:1] == ['prefix-check'] and len(args) in (5, 6) and args[1] in ('libs', 'data', 'summary'):
                service = Provision()
                service.options(['--cells', args[2]])
                service.pin = Path(args[4])
                prefix = Path(args[3])
                if args[1] == 'libs':
                    return 0 if service.libs_ok(args[2], prefix) else PREFIX_INVALID
                if args[1] == 'data':
                    return 0 if service.data_ok(args[2], prefix) else PREFIX_INVALID
                print(service.summary(args[2], prefix, args[5] if len(args) == 6 else None), flush=True)
                return 0
            if args[:1] == ['data-check'] and len(args) == 3:
                service = Provision()
                service.options(['--cells', args[1]])
                return 0 if service.data_ok(args[1], args[2]) else 1
            service = Provision()
            return service.execute(args)
    except Finished as error:
        return error.status
    except slot.Cancelled:
        return 1 if service is not None and service.custom_ca else slot.cancellation_status()
    except OSError as error:
        print(f'FAIL: {error.strerror}', file=sys.stderr)
        return 1
    except (ValueError, doctor.DoctorError) as error:
        print(f'FAIL: {error}', file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
