"""Post-bootstrap cloud provisioning and shell-readable oracle environments.

The shell entry point owns only uv/interpreter bootstrap. Compatibility identity,
loader, cancellable process and bounded pool services remain their sole owners.
Reference images receive build-oracle.sh and pin inputs, never tools.dev.
"""
from contextlib import ExitStack
import hashlib
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import time

from . import doctor, slot
from .compat import images
from .compat.identity import digest_file
from .compat.process import ROOT, capture, fail, run, text
from .runners import Finished
from tools.tkrzw import runtime

CELLS = ('bdb', 'kc', 'tkrzw')
TKRZW_REV = '9ee84164cf45e441aa8186418c03a02c9b89f08b'
TKRZW_URL = 'https://github.com/estraier/tkrzw'
SELECT = '''case $_oxp_provisioned in
    *" $PINYIN_ORACLE_DBM "*) ;;
    *) echo "cell $PINYIN_ORACLE_DBM was not provisioned by this environment file" >&2; unset _oxp_provisioned; return 1 ;;
esac
_oxp_cell=${PINYIN_ORACLE_DBM^^}
_oxp_prefix=OXPINYIN_ORACLE_PREFIX_$_oxp_cell
_oxp_data=OXPINYIN_ORACLE_DATA_$_oxp_cell
export PINYIN_ORACLE_PREFIX=${!_oxp_prefix:?cell not provisioned by tools/cloud/setup.sh}
export OXPINYIN_LIBPINYIN_DATA_DIR=${!_oxp_data}
unset _oxp_cell _oxp_prefix _oxp_data _oxp_provisioned
'''


def quote(value, *, always=False):
    """Serialize Bash %q words and @Q path prefixes without evaluating values."""
    value = str(value)
    if not value:
        return "''"
    controls = {'\a': r'\a', '\b': r'\b', '\t': r'\t', '\n': r'\n',
                '\v': r'\v', '\f': r'\f', '\r': r'\r', '\x1b': r'\E'}
    if any(ord(char) < 32 or ord(char) == 127 for char in value):
        escaped = ''.join(controls.get(char, '\\' + char if char in "'\\" else
                                      r'\177' if ord(char) == 127 else
                                      f'\\{ord(char):03o}' if ord(char) < 32 else char) for char in value)
        return "$'" + escaped + "'"
    if always:
        return "'" + value.replace("'", "'\\''") + "'"
    special = set(" ,'" + '"' + '\\' + '`$&|;()<>!*?[]{}')
    return ''.join('\\' + char if char in special or char == '#' and index == 0 or
                   char == '~' and (index == 0 or value[index - 1] in '=:') else char
                   for index, char in enumerate(value))


def assignments(path, names):
    """Read only generated assignments, omitting the environment selector.

    Legacy printf %q files use backslash or ANSI-C quoting. Bash sources only
    their assignment words; membership/selector and arbitrary lines never run.
    The caller supplies the allowlisted variable names and an empty inheritance.
    """
    selected = []
    if Path(path).is_file():
        for line in Path(path).read_text().splitlines():
            match = re.fullmatch(r'(?:export )?([A-Za-z_][A-Za-z_0-9]*)=(.*)', line)
            if match and match[1] in names:
                selected.append(line)
    if not selected:
        return {}
    # Existing files are setup-generated shell source. Keep its quoting and
    # intentional empty loader inheritance when decoding those exact fields.
    script = '\n'.join(selected) + '\n' + '\n'.join(
        'printf "%s\\0" "${' + name + '-}"' for name in names)
    done = capture(['bash', '-c', script], env={'PATH': os.environ.get('PATH', '')})
    return dict(zip(names, done.stdout.decode().split('\0')[:-1]))


def tkrzw_env_prefix(path):
    """Read a cache prefix, including pre-OXPINYIN_TKRZW_PREFIXES files."""
    path = Path(path)
    if not path.is_file():
        return ''
    lines = path.read_text().splitlines()
    if any(line.startswith('export OXPINYIN_TKRZW_PREFIX=') for line in lines):
        return assignments(path, ['OXPINYIN_TKRZW_PREFIX']).get('OXPINYIN_TKRZW_PREFIX', '')
    if any(line.startswith('export OXPINYIN_TKRZW_LIBRARY=') for line in lines):
        return ''
    lib = assignments(path, ['LD_LIBRARY_PATH']).get('LD_LIBRARY_PATH', '').split(':')[0]
    return lib[:-4] if lib.endswith('/lib') else ''


def without_prefix(value, prefix, env):
    """Remove every sourced cache and the current cache from a host search path."""
    excluded = [*env.get('OXPINYIN_TKRZW_PREFIXES', '').split(':'), str(prefix or ''),
                tkrzw_env_prefix(env.get('OXPINYIN_ORACLE_ENV', str(Path.home() / '.oxpinyin-oracle.env')))]
    # Bash keeps leading/internal empty fields and discards a final empty field.
    entries = value.split(':') if value else []
    if value.endswith(':'):
        entries.pop()
    return ':'.join(entry for entry in entries
                    if not any(p and (entry == p or entry.startswith(p + '/')) for p in excluded))


def resolve_tkrzw(obj, env, root=ROOT):
    """Use the shared loader's historical host-probe view and exact statuses."""
    try:
        host, done, matches = runtime.resolve(obj, env=env, cwd=root, probe=True)
    except runtime.LoaderOverride:
        fail('REFUSED: otool -L reports the recorded libtkrzw install name, which DYLD_LIBRARY_PATH or DYLD_FALLBACK_LIBRARY_PATH can bypass; unset both to probe the host library.')
    except runtime.LoaderFailure as error:
        raise Finished(error.status) from error
    if done.returncode:
        raise Finished(128 - done.returncode if done.returncode < 0 else done.returncode)
    return '\n'.join(matches[:1] if host == 'Darwin' else matches)


def probe_host(prefix='', *, env=None, root=ROOT, probe=None, out=None):
    """Certify the host utility and loaded object after excluding all caches."""
    def refused(reason):
        print(f'host check: {reason}; no healthy host libtkrzw was certified',
              file=sys.stderr, flush=True)
        return ''

    environment = dict(os.environ if env is None else env)
    child = dict(environment, PATH=without_prefix(environment.get('PATH', ''), prefix, environment),
                 LD_LIBRARY_PATH=without_prefix(environment.get('LD_LIBRARY_PATH', ''), prefix, environment))
    status = run(['sh', probe or Path(root) / 'tools/tkrzw/distro-probe.sh'], env=child,
                 cwd=root, stdout=out, check=False)
    if status:
        return refused(f'host distro-probe.sh failed (status {status})')
    util = shutil.which('tkrzw_dbm_util', path=child['PATH'])
    if not util:
        return refused('tkrzw_dbm_util is not on the host PATH after excluding cached runtimes')
    try:
        so = resolve_tkrzw(util, child, root)
    except Finished as error:
        return refused(f'the host utility loader check failed (status {error.status})')
    libname = 'libtkrzw.dylib' if text(['uname', '-s'], env=child) == 'Darwin' else 'libtkrzw.so.1'
    print(f'host check: {util} resolves {libname} to {so or "nothing"}', file=out or sys.stdout, flush=True)
    if not so:
        return refused('the host utility did not resolve libtkrzw')
    if not Path(so).is_file():
        return refused(f'resolved host libtkrzw is not a file: {so}')
    if without_prefix(so, prefix, environment) != so:
        return refused(f'resolved libtkrzw belongs to an excluded cache: {so}')
    real = str(Path(so).resolve())
    if without_prefix(real, prefix, environment) != real:
        return refused(f'resolved host libtkrzw points into an excluded cache: {real}')
    return real


class Provision:
    """One invocation's paths, qualification state, and provisioning phases."""

    def __init__(self, *, root=ROOT, env=None):
        self.root = Path(root)
        self.env = dict(os.environ if env is None else env)
        self.pin = self.root / 'tools/oracle/oracle-pin.txt'
        self.cache = Path(self.env.get('OXPINYIN_CLOUD_CACHE') or
                          str(Path(self.env.get('XDG_CACHE_HOME') or str(Path.home() / '.cache')) / 'oxpinyin'))
        self.cells = list(CELLS)
        try:
            self.jobs = text(['nproc'], env=self.env, check=False, stderr=False) or '2'
        except OSError:
            self.jobs = '2'
        self.pull, self.inside, self.python_only = True, False, False
        self.platform = ''
        self.started = time.monotonic() - int(self.env.get('OXPINYIN_CLOUD_BOOTSTRAP_SECONDS', '0'))
        self.tkrzw_local = False
        self.tkrzw_host_library = ''
        self.ctr = ''
        self.custom_ca = False
        self.empty_cache = False
        self.net_flags, self.env_flags = ['--network', 'host'], []
        self.uv_dir = str(Path(shutil.which('uv', path=self.env.get('PATH')) or 'uv').parent)

    @property
    def oracle_root(self):
        return self.cache / 'oracle'

    @property
    def tkrzw_prefix(self):
        return self.cache / 'tkrzw'

    @property
    def env_file(self):
        return Path(self.env.get('OXPINYIN_ORACLE_ENV') or str(Path.home() / '.oxpinyin-oracle.env'))

    def log(self, message):
        print(f'[{int(time.monotonic() - self.started):4d}s] {message}', file=sys.stderr, flush=True)

    def die(self, message):
        fail('FAIL: ' + message)

    def field(self, key):
        return images.platform_field(self.pin, self.platform, key)

    def options(self, argv):
        """Preserve setup's options, exclusions, platform and cell checks."""
        args, oracle_options = list(argv), False
        while args:
            option = args.pop(0)
            if option in ('-h', '--help'):
                raise Finished(0)  # Historical setup.sh's header extraction prints nothing.
            if option == '--python-only':
                self.python_only = True
            elif option in ('--cache-dir', '--cells', '--jobs'):
                oracle_options = True
                if not args:
                    fail(f'{option}: requires an argument', 1)
                value = args.pop(0)
                if option == '--cache-dir':
                    self.cache = Path(value)
                    self.empty_cache = not value
                elif option == '--cells':
                    self.cells = re.findall(r'[^ \t]+', value.partition('\n')[0])
                else:
                    self.jobs = value
            elif option in ('--in-container', '--no-pull'):
                oracle_options = True
                if option == '--in-container':
                    self.inside = True
                else:
                    self.pull = False
            else:
                fail(f'unknown option: {option}', 2)
        if self.python_only and oracle_options:
            fail('--python-only cannot be combined with --cells, --cache-dir, --jobs, --no-pull or --in-container', 2)
        if text(['uname', '-s'], env=self.env) != 'Linux':
            fail('Oracle provisioning requires Linux; run subject, reference and harness together in a Linux environment. See docs/runbooks/setup.md.', 2)
        arch = text(['uname', '-m'], env=self.env)
        self.platform = {'x86_64': 'linux/amd64', 'aarch64': 'linux/arm64', 'arm64': 'linux/arm64'}.get(arch, '')
        if not self.platform:
            fail(f'No frozen oracle hashes for architecture {arch}; supported platforms are linux/amd64 and linux/arm64. See docs/runbooks/setup.md.', 2)
        if not self.field('libpinyin_sha256_bdb'):
            fail(f'No frozen oracle hashes for {self.platform} in {self.pin}. See docs/runbooks/setup.md.', 2)
        if self.inside and (self.env.get('OXPINYIN_COMPAT_CONTAINER') != '1' or not Path('/opt/reference').is_dir()):
            fail('--in-container requires the controlled compatibility image; no host provisioning attempted', 2)
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
        if self.python_only:
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
        if self.inside:
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

    def report_ext4(self):
        """Report writable candidates without claiming measurement readiness."""
        if not self.which('findmnt'):
            print('ext4 paths: unknown (findmnt unavailable); measurement gates require a verified ext4 path.')
            return
        done = capture(['findmnt', '--raw', '--noheadings', '--output', 'TARGET,FSTYPE'], env=self.env, check=False, stderr=False)
        unknown, found = bool(done.status), False
        candidates = [str(self.root), self.env.get('HOME', str(Path.home())), self.env.get('TMPDIR') or '/tmp', str(self.cache)]
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

    def libs_ok(self, cell, prefix=None):
        prefix = Path(prefix) if prefix is not None else self.oracle_root / cell
        return all((path := prefix / 'lib' / (lib + '.so')).is_file() and
                   digest_file(path) == self.field(f'{lib}_sha256_{cell}') for lib in ('libpinyin', 'libzhuyin'))

    def data_ok(self, cell, prefix=None):
        """Check stable pin identity and prefix-local generated manifest provenance."""
        prefix = Path(prefix) if prefix is not None else self.oracle_root / cell
        stable = prefix / 'oracle-data.sha256'
        if not stable.is_file() or digest_file(stable) != self.field(f'data_manifest_sha256_{cell}'):
            return False
        with open(os.devnull, 'wb') as null:
            if run(['sha256sum', '--check', '--quiet', stable.name], cwd=prefix, env=self.env,
                   stdout=null, stderr=null, check=False):
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
        with tempfile.TemporaryFile() as output:
            status = run(['sha256sum', '--check', '--quiet', unstable.name], cwd=prefix, env=self.env,
                         stdout=output, stderr=subprocess.STDOUT, check=False)
            output.seek(0)
            sys.stderr.write(output.read().decode())
        return not status

    def write_oracle_env(self):
        """Retain only verified cells and atomically publish one sourceable file."""
        path = self.env_file
        env_cells = list(self.cells)
        prefixes = {cell: str(self.oracle_root / cell) for cell in env_cells}
        names = ['_oxp_provisioned', *(f'OXPINYIN_ORACLE_{kind}_{cell.upper()}' for cell in CELLS for kind in ('PREFIX', 'DATA'))]
        previous = assignments(path, names)
        kept_paths, kept_library = '', ''
        for cell in CELLS:
            if cell in prefixes or f' {cell} ' not in previous.get('_oxp_provisioned', ''):
                continue
            prefix = previous.get(f'OXPINYIN_ORACLE_PREFIX_{cell.upper()}', '')
            if not prefix or not self.libs_ok(cell, prefix):
                self.log(f'{cell}: removed from environment file; oracle-pin.txt library hash check failed')
            elif not self.data_ok(cell, prefix):
                self.log(f'{cell}: removed from environment file; data manifest check failed')
            else:
                prefixes[cell] = prefix
                env_cells.append(cell)
                if cell == 'tkrzw':
                    lines, taking = [], False
                    for line in path.read_text().splitlines():
                        if line.startswith('export PKG_CONFIG_PATH='):
                            taking = True
                        if taking:
                            lines.append(line)
                        if taking and line.startswith('export PATH='):
                            taking = False
                    kept_paths = '\n'.join(lines)
                    kept_library = assignments(path, ['OXPINYIN_TKRZW_LIBRARY']).get('OXPINYIN_TKRZW_LIBRARY', '')
        default = 'bdb' if 'bdb' in env_cells else env_cells[0]
        runtime_prefix = str(self.tkrzw_prefix) if self.tkrzw_local else ''
        library = self.tkrzw_host_library
        if not runtime_prefix and not library and kept_paths:
            runtime_prefix = tkrzw_env_prefix(path)
        if runtime_prefix:
            obj = Path(runtime_prefix) / 'lib/libtkrzw.so.1'
            library = str(obj.resolve()) if obj.exists() else ''
        elif not library:
            library = kept_library
        lines = ['# Written by tools/cloud/setup.sh; source it, do not edit it.',
                 '_oxp_provisioned=' + quote(' ' + ' '.join(env_cells) + ' '),
                 'export PINYIN_MODEL_DIR=' + quote(self.model_cache / 'extracted'),
                 'export PATH=' + quote(self.uv_dir, always=True) + ':$PATH']
        if library:
            lines.append('export OXPINYIN_TKRZW_LIBRARY=' + quote(library))
        if runtime_prefix:
            lines += ['export OXPINYIN_TKRZW_PREFIX=' + quote(runtime_prefix),
                      'export OXPINYIN_TKRZW_PREFIXES=' + quote(runtime_prefix, always=True) + '${OXPINYIN_TKRZW_PREFIXES:+:$OXPINYIN_TKRZW_PREFIXES}']
        if self.tkrzw_local:
            lines += ['export PKG_CONFIG_PATH=' + quote(self.tkrzw_prefix, always=True) + '/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}',
                      'export LD_LIBRARY_PATH=' + quote(self.tkrzw_prefix, always=True) + '/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}',
                      'export PATH=' + quote(self.tkrzw_prefix, always=True) + '/bin:$PATH']
        elif library and not runtime_prefix:
            lines.append('export LD_LIBRARY_PATH=' + quote(Path(library).parent, always=True) + '${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}')
        elif kept_paths:
            lines.append(kept_paths)
        for cell in env_cells:
            lines += [f'export OXPINYIN_ORACLE_PREFIX_{cell.upper()}=' + quote(prefixes[cell]),
                      f'export OXPINYIN_ORACLE_DATA_{cell.upper()}=' + quote(prefixes[cell] + '/lib/libpinyin/data')]
        lines += ['export PINYIN_ORACLE_DBM=${PINYIN_ORACLE_DBM:-' + default + '}', SELECT.rstrip('\n')]
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
        elif self.proxy and not self.inside:
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
        if self.custom_ca and not self.inside:
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
            run(['sleep', str(delay)], env=self.env)
        run(command, env=self.env)

    def backend_tools(self):
        missing = [(tool, package) for cell, tool, package in
                   (('bdb', 'db5.3_dump', 'db5.3-util'), ('kc', 'kchashmgr', 'kyotocabinet-utils'))
                   if cell in self.cells and not self.which(tool)]
        if missing:
            tools, packages = map(list, zip(*missing))
            if os.geteuid() == 0:
                self.log(f'installing {" ".join(packages)} from the distribution archive')
                if not self.apt(packages):
                    self.die(f'could not install {" ".join(packages)} with apt-get')
            else:
                fail(f'missing: {" ".join(tools)}\nrun as root: apt-get install -y {" ".join(packages)}')

    def runtime_ready(self):
        def ready():
            with open(os.devnull, 'wb') as null:
                return not run([self.ctr, 'info'], env=self.env, stdout=null, stderr=null, check=False)
        if ready():
            self.log(f'{self.ctr} ready')
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
            run([self.ctr, 'info'], env=self.env, stdout=sys.stderr, check=False)
            self.die(f'{self.ctr} info failed; there is no daemon to start for it')

    def tail(self, path, count=5, logged=False):
        for line in Path(path).read_text(errors='replace').splitlines()[-count:]:
            self.log('  ' + line) if logged else print(line, file=sys.stderr)

    def build_deps_image(self):
        if self.inside:
            return
        with open(os.devnull, 'wb') as null:
            if not run([self.ctr, 'image', 'inspect', self.deps_image], env=self.env, stdout=null, stderr=null, check=False):
                return
            if self.pull:
                with (self.cache / 'pull-deps.err').open('wb') as error:
                    status = run([self.ctr, 'pull', '--quiet', '--platform', self.platform, self.published_deps], env=self.env, stdout=null, stderr=error, check=False)
                if not status:
                    run([self.ctr, 'tag', self.published_deps, self.deps_image], env=self.env)
                    return
                self.log(f'recorded deps image unavailable; building locally (details: {self.cache}/pull-deps.err)')
                self.tail(self.cache / 'pull-deps.err')
            if run([self.ctr, 'image', 'inspect', self.base_image], env=self.env, stdout=null, stderr=null, check=False):
                self.log(f'pulling {self.base_image} for the local fallback build')
                self.retry([self.ctr, 'pull', '--quiet', '--platform', self.platform, self.base_image])
        self.log(f'building {self.deps_image} (the trixie pin-reference toolchain)')
        flags = list(self.net_flags)
        if self.platform != 'linux/amd64':
            flags += ['--build-arg', f'ORACLE_PLATFORM={self.platform}']
        if self.proxy:
            flags += ['--build-arg', 'https_proxy', '--build-arg', 'HTTPS_PROXY']
            if self.custom_ca:
                flags += ['--secret', f'id=cacert,src={self.ca}']
        run([self.ctr, 'build', '--platform', self.platform, *flags, '--target', 'deps',
             '-f', self.root / 'tools/oracle/Dockerfile.oracle', '-t', self.deps_image, self.root],
            env=dict(self.env, DOCKER_BUILDKIT='1', https_proxy=self.proxy, HTTPS_PROXY=self.proxy), stdout=2, heavy=True)

    def in_image_command(self, args, flags=()):
        """Keep pre-provisioned compat execution outside the native runtime path."""
        if self.inside:
            return [str(self.root / 'tools/oracle/build-oracle.sh'), *args]
        return [self.ctr, 'run', '--rm', '--platform', self.platform, *self.net_flags, *self.env_flags, *flags,
                '-v', f'{self.root}/tools/oracle/build-oracle.sh:/usr/local/bin/build-oracle.sh:ro,z',
                '-v', f'{self.pin}:/usr/local/bin/oracle-pin.txt:ro,z',
                '-v', f'{self.oracle_root}:/opt/pinyin-oracle:z', '-v', f'{self.model_cache}:/model-cache:ro,z',
                self.deps_image, 'build-oracle.sh', *args]

    def pool_phase(self, label, cells, log, args):
        """Reuse #1233's exact pool phase and owned-container cancellation."""
        flags, owner = [], []
        if not self.inside:
            flags = ['--name', f'oxpinyin-setup-{{token}}-{label}-{{item}}', '--label', 'oxpinyin.setup={token}',
                     '--cidfile', '{cidfile}', '--cpus', '{jobs}']
            owner = ['--container-runtime', shlex.quote(self.ctr), '--container-label', 'oxpinyin.setup']
        # A subprocess preserves the invocation's environment without mutating
        # Python's process-global environ during pool scheduling.
        return run([sys.executable, '-X', 'lazy_imports=normal', '-m', 'tools.dev', 'pool', 'run',
                    '--label', label, '--items', ','.join(cells), '--cpus', str(self.jobs),
                    '--log', str(log), *owner, '--', *self.in_image_command(args, flags)],
                   cwd=self.root, env=self.env, check=False)

    def libraries(self):
        if self.inside:
            for cell in self.cells:
                if not self.libs_ok(cell):
                    shutil.rmtree(self.oracle_root / cell, ignore_errors=True)
                    shutil.copytree(Path('/opt/reference') / cell, self.oracle_root / cell, symlinks=True)
                if not self.libs_ok(cell):
                    for library in ('libpinyin', 'libzhuyin'):
                        print(f'MISMATCH {cell} {library}\n  recorded: {self.field(f"{library}_sha256_{cell}")}\n  reference: {digest_file(self.oracle_root / cell / "lib" / (library + ".so"))}', file=sys.stderr)
                    raise Finished(3)
        need = []
        for cell in self.cells:
            if self.libs_ok(cell):
                self.log(f'{cell}: libraries already match oracle-pin.txt')
            else:
                need.append(cell)
        if need and self.pull:
            self.log(f'trying {self.published}')
            error_path = self.cache / 'pull-published.err'
            with error_path.open('wb') as error, open(os.devnull, 'wb') as null:
                status = run([self.ctr, 'pull', '--quiet', '--platform', self.platform, self.published], env=self.env, stdout=null, stderr=error, check=False)
            if not status:
                cid = text([self.ctr, 'create', '--platform', self.platform, self.published, '/none'], env=self.env)
                try:
                    for cell in need:
                        shutil.rmtree(self.oracle_root / cell, ignore_errors=True)
                        run([self.ctr, 'cp', f'{cid}:/opt/pinyin-oracle/{cell}', self.oracle_root / cell], env=self.env, stdout=subprocess.DEVNULL)
                finally:
                    run([self.ctr, 'rm', cid], env=self.env, stdout=subprocess.DEVNULL, check=False)
                still = []
                for cell in need:
                    if self.libs_ok(cell):
                        self.log(f'{cell}: pulled libraries match oracle-pin.txt')
                    else:
                        self.log(f'{cell}: pulled libraries do not match oracle-pin.txt; building instead')
                        shutil.rmtree(self.oracle_root / cell, ignore_errors=True)
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
                shutil.rmtree(self.oracle_root / cell, ignore_errors=True)
            if self.pool_phase('libraries', need, self.cache / 'build-{item}.log',
                               ['--libs-only', '--enable-libzhuyin', '--dbm', '{item}', '--work-dir', '/build/{item}',
                                '--prefix', '/opt/pinyin-oracle/{item}', '--jobs', '{jobs}']):
                self.die(f'the library build failed or was cancelled (logs: {self.cache}/build-<cell>.log)')
        mismatch = False
        for cell in self.cells:
            for library in ('libpinyin', 'libzhuyin'):
                want = self.field(f'{library}_sha256_{cell}')
                got = digest_file(self.oracle_root / cell / 'lib' / (library + '.so'))
                if got != want:
                    print(f'MISMATCH {cell} {library}\n  recorded: {want}\n  built:    {got}', file=sys.stderr)
                    mismatch = True
        if mismatch:
            print(f'The recipe did not reproduce the recorded hashes on this host; the built\nlibraries are left in {self.oracle_root} for readelf/cmp against a reference.', file=sys.stderr)
            raise Finished(3)

    def sound_tkrzw(self):
        if 'tkrzw' not in self.cells:
            return
        probe = self.root / 'tools/tkrzw/distro-probe.sh'
        prefix = self.tkrzw_prefix
        child = dict(self.env, PATH=str(prefix / 'bin') + ':' + self.env.get('PATH', ''),
                     LD_LIBRARY_PATH=str(prefix / 'lib') + (':' + self.env['LD_LIBRARY_PATH'] if self.env.get('LD_LIBRARY_PATH') else ''))
        def built_ok():
            done = capture(['sh', str(probe)], env=child, check=False, stderr=False)
            if done.status or b'RESULT : healthy' not in done.stdout + done.stderr:
                return False
            try:
                return resolve_tkrzw(prefix / 'bin/tkrzw_dbm_util', child, self.root) == str(prefix / 'lib/libtkrzw.so.1')
            except Finished:
                return False
        # Redirect both Python and child probe streams to this run's local file.
        command = [sys.executable, '-X', 'lazy_imports=normal', '-m', 'tools.dev', 'provision', 'probe-host', str(prefix)]
        with (self.cache / 'tkrzw-host-probe.log').open('wb') as output:
            done = capture(command, cwd=self.root, env=self.env, check=False, stderr=False)
            output.write(done.stdout + done.stderr)
        if not done.status:
            self.tkrzw_host_library = done.stdout.decode().splitlines()[-1]
            self.log('tkrzw: the host libtkrzw passes distro-probe.sh')
            return
        self.log(f'tkrzw: the host libtkrzw is broken or missing (probe: {self.cache}/tkrzw-host-probe.log)')
        self.tkrzw_local = True
        revision = prefix / 'REV'
        if revision.is_file() and revision.read_text().rstrip('\n') == TKRZW_REV and built_ok():
            self.log(f'tkrzw: cached build {TKRZW_REV[:7]} passes distro-probe.sh')
        else:
            for name in ('g++', 'make'):
                if not self.which(name):
                    self.die(f'building tkrzw needs {name} (apt-get install -y build-essential)')
            self.log(f'tkrzw: building upstream {TKRZW_REV[:7]}')
            source = self.cache / 'tkrzw-src'
            shutil.rmtree(prefix, ignore_errors=True)
            shutil.rmtree(source, ignore_errors=True)
            source.mkdir(parents=True)
            # All compilation, including the local runtime, uses slot.run_command
            # through compat.process; the build remains one sequential unit.
            status = run([sys.executable, '-X', 'lazy_imports=normal', '-m', 'tools.dev', 'provision',
                          'build-tkrzw', str(source), str(prefix), str(self.jobs)], cwd=self.root,
                         env=self.env, stdout=(output := (self.cache / 'tkrzw-build.log').open('wb')), heavy=True, check=False)
            output.close()
            if status:
                self.tail(self.cache / 'tkrzw-build.log', 40)
                self.die(f'the tkrzw build failed (full log: {self.cache}/tkrzw-build.log)')
            shutil.rmtree(source)
            if not built_ok():
                run(['sh', str(probe)], env=dict(child, LD_LIBRARY_PATH=str(prefix / 'lib')), stdout=sys.stderr, check=False)
                self.die("the built tkrzw does not print 'RESULT : healthy' (refusing to use it)")
            revision.write_text(TKRZW_REV + '\n')
        resolved = resolve_tkrzw(self.oracle_root / 'tkrzw/lib/libpinyin.so', dict(self.env, LD_LIBRARY_PATH=str(prefix / 'lib')), self.root)
        if resolved != str(prefix / 'lib/libtkrzw.so.1'):
            self.die(f"the tkrzw oracle libpinyin.so resolves libtkrzw.so.1 to '{resolved or 'nothing'}', not {prefix}/lib")
        self.log(f'tkrzw: oracle libpinyin.so resolves libtkrzw.so.1 to {resolved}')

    def execute(self, argv):
        self.options(argv)
        self.prerequisites()
        if self.env.get('OXPINYIN_CLOUD_BOOTSTRAP_KEPT_UV', '1') == '1':
            self.log('using ' + text(['uv', '--version'], env=self.env))
        # Bootstrap selected the managed interpreter. Doctor owns its policy;
        # setup retains the historical successful verification line.
        identity = doctor.identity(self.env)
        print('Verified managed CPython', identity['version'], 'GIL build:', identity['executable'], flush=True)
        if self.python_only:
            self.report_ext4()
            self.log('stopping after Python provisioning (--python-only)')
            return 0
        with ExitStack() as stack:
            self.proxy_ca(stack)
            if self.empty_cache:
                run(['mkdir', '-p', ''], env=self.env)
            self.cache.mkdir(parents=True, exist_ok=True)
            self.cache = self.cache.resolve()
            self.model_cache = Path(self.env.get('PINYIN_MODEL_CACHE') or str(self.cache / 'model20'))
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
            for path in ('tools/oracle/Dockerfile.oracle', 'tools/oracle/versions.py', 'tools/oracle/version-manifest.json', 'tools/tkrzw/distro-probe.sh', 'tools/oracle/oracle-pin.txt'):
                digest.update((self.root / path).read_bytes())
            self.deps_image = 'oxpinyin-oracle-deps:' + digest.hexdigest()[:12]
            if not self.inside:
                self.ctr = images.select_runtime(env=self.env)
                if Path(self.ctr).name == 'container':
                    self.die('normal cloud provisioning requires a Linux cloud runtime; use compat.sh for the Apple adapter')
            else:
                print('Runtime: supplied compatibility container (no nested runtime)')
            self.backend_tools()
            for cell in self.cells:
                if not self.libs_ok(cell) or not self.data_ok(cell):
                    if run([self.root / 'tools/model/fetch-model.sh', '--cache-dir', self.model_cache], env=self.env, stdout=subprocess.DEVNULL, check=False):
                        self.die('model20 is unavailable; allow downloads.sourceforge.net and its *.dl.sourceforge.net redirect target, or supply a verified local model cache (docs/runbooks/cloud.md)')
                    break
            if self.inside:
                Path('/opt/pinyin-oracle').symlink_to(self.oracle_root)
                Path('/model-cache').symlink_to(self.model_cache)
            else:
                self.runtime_ready()
            self.libraries()
            self.sound_tkrzw()
            need = []
            for cell in self.cells:
                if self.data_ok(cell):
                    self.log(f'{cell}: data already matches the recorded manifest')
                else:
                    need.append(cell)
            if need:
                self.log(f'fetching model20 for {" ".join(need)}')
                run([self.root / 'tools/model/fetch-model.sh', '--cache-dir', self.model_cache], env=self.env, stdout=subprocess.DEVNULL)
                self.build_deps_image()
                if self.pool_phase('data', need, self.cache / 'data-{item}.log',
                                   ['--regen-data', '--dbm', '{item}', '--prefix', '/opt/pinyin-oracle/{item}',
                                    '--model-dir', '/model-cache/extracted', '--work-dir', '/build/{item}-data', '--jobs', '{jobs}']):
                    self.die(f'--regen-data failed or was cancelled (logs: {self.cache}/data-<cell>.log)')
                for cell in need:
                    if not self.data_ok(cell):
                        self.die(f'{cell}: the regenerated data does not match the recorded manifest')
            self.report_ext4()
            if not self.inside:
                print(f'Valgrind: {self.valgrind_version}')
            for cell in self.cells:
                prefix = self.oracle_root / cell
                for pc in (prefix / 'lib/pkgconfig').glob('*.pc'):
                    pc.write_text(re.sub(r'^prefix=.*$', lambda _: 'prefix=' + str(prefix), pc.read_text(), flags=re.M))
                print(f'{cell} prefix={prefix} libpinyin={digest_file(prefix / "lib/libpinyin.so")} libzhuyin={digest_file(prefix / "lib/libzhuyin.so")} data manifest OK', flush=True)
            self.write_oracle_env()
        return 0


def main(argv=None):
    """Dispatch cloud provisioning after bootstrap, including native host callers."""
    args = list(sys.argv[1:] if argv is None else argv)
    service = None
    try:
        with slot.cancellation_handlers():
            if args[:1] == ['probe-host']:
                library = probe_host(args[1] if len(args) > 1 else '')
                if not library:
                    return 1
                print(library)
                return 0
            if args[:1] == ['data-check'] and len(args) == 3:
                service = Provision()
                service.options(['--cells', args[1]])
                return 0 if service.data_ok(args[1], args[2]) else 1
            if args[:1] == ['build-tkrzw'] and len(args) == 4:
                os.dup2(1, 2)
                source, prefix, jobs = args[1:]
                service = Provision()
                run(['git', 'init', '-q', '.'], cwd=source)
                # Fetch/retry is the shared launch adapter; no new controller.
                for delay in (2, 4, 8, 16, None):
                    if not run(['git', 'fetch', '-q', '--depth=1', TKRZW_URL, TKRZW_REV], cwd=source, check=False):
                        break
                    if delay is None:
                        raise Finished(1)
                    service.log(f'retrying in {delay}s: git fetch -q --depth=1 {TKRZW_URL} {TKRZW_REV}')
                    run(['sleep', str(delay)])
                run(['git', 'checkout', '-q', 'FETCH_HEAD'], cwd=source)
                env = {k: v for k, v in os.environ.items() if k not in ('CFLAGS', 'CXXFLAGS', 'LDFLAGS')}
                run(['./configure', '--prefix=' + prefix], cwd=source, env=env)
                run(['make', '-j' + jobs], cwd=source)
                run(['make', 'install'], cwd=source)
                return 0
            service = Provision()
            return service.execute(args)
    except Finished as error:
        return error.status
    except slot.Cancelled:
        return 1 if service is not None and service.custom_ca and not service.inside else slot.cancellation_status()
    except OSError as error:
        print(f'FAIL: {error.strerror}', file=sys.stderr)
        return 1
    except (ValueError, doctor.DoctorError) as error:
        print(f'FAIL: {error}', file=sys.stderr)
        return 1
