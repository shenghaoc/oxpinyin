"""Backend settings shared by the surviving ABI, packaging, and test paths."""

from collections.abc import Mapping
from dataclasses import dataclass
import functools
from pathlib import Path
import platform
import re
import tomllib

from . import slot

ROOT = Path(__file__).resolve().parents[2]
BACKENDS = ('bdb', 'kyotocabinet', 'tkrzw')
CELL_TO_BACKEND = {'bdb': 'bdb', 'kc': 'kyotocabinet', 'tkrzw': 'tkrzw'}
BACKEND_TO_CELL = {backend: cell for cell, backend in CELL_TO_BACKEND.items()}
CELLS = tuple(CELL_TO_BACKEND)
BACKEND_FORMATS = {'bdb': 'BerkeleyDB', 'kyotocabinet': 'KyotoCabinet', 'tkrzw': 'Tkrzw'}
BACKEND_EXTENSIONS = {'bdb': 'db', 'kyotocabinet': 'kct', 'tkrzw': 'tkt'}
# Linux runs Kyoto Cabinet; the default tkrzw backend is exercised on macOS and
# Windows because the Ubuntu tkrzw runtime is not supported.
LOCAL_CELL = 'kc'


class CellError(ValueError):
    """An invalid backend setting or missing ABI build artifact."""

    def __init__(self, message: str, status: int = 1):
        super().__init__(message)
        self.status = status


@functools.cache
def _declared_default(root: Path) -> str:
    manifest = root / 'Cargo.toml'
    defaults = tomllib.loads(manifest.read_text()).get('features', {}).get('default') if manifest.is_file() else None
    if not isinstance(defaults, list) or len(defaults) != 1 or defaults[0] not in BACKENDS:
        raise ValueError(f'oxpinyin needs one backend default: {defaults}')
    return defaults[0]


def declared_default(root: Path = ROOT) -> str:
    """Read the backend default from the root package manifest."""
    return _declared_default(root.resolve())


def default_cell(root: Path = ROOT) -> str:
    return BACKEND_TO_CELL[declared_default(root)]


def host_cell(root: Path = ROOT) -> str:
    """Return Linux's Kyoto Cabinet cell, or the package default elsewhere."""
    return LOCAL_CELL if platform.system() == 'Linux' else default_cell(root)


def feature_flags(backend: str | None = None) -> list[str]:
    """Return Cargo feature flags for one explicit backend."""
    if backend is None:
        return []
    if backend not in BACKENDS:
        raise ValueError(f'unknown backend: {backend}')
    return ['--no-default-features', '--features', backend]


def _word(value: str) -> str:
    if not value:
        return "''"
    return value if value.replace('_', '').replace('-', '').isalnum() else repr(value)


@dataclass(frozen=True)
class Settings:
    """The backend-specific Cargo environment for one surviving tool invocation."""

    root: Path
    cell: str
    feature: str
    extension: str
    dbm_name: str
    expected_pin: str
    target: Path
    environment: Mapping[str, str]

    def feature_args(self) -> list[str]:
        return feature_flags(self.feature)

    @classmethod
    def read(cls, environment: Mapping[str, str], root: Path = ROOT) -> 'Settings':
        root = root.resolve()
        cell = environment.get('PINYIN_ORACLE_DBM') or host_cell(root)
        if cell not in CELL_TO_BACKEND:
            raise CellError(f'fatal: PINYIN_ORACLE_DBM={_word(cell)} is not one of: bdb kc tkrzw', 3)
        feature = CELL_TO_BACKEND[cell]
        source = root / 'tools/oracle/oracle-pin.txt'
        try:
            pin = dict(re.findall(r'^(libpinyin_tag|libpinyin_commit|model_archive_sha256)=(.+)$',
                                  source.read_text(), re.M))
        except OSError:
            pin = {}
        if len(pin) != 3:
            raise CellError(f'fatal: could not read the frozen pin from {source}')
        expected = (f'pin_ref=libpinyin-{pin["libpinyin_tag"]}-'
                    f'{pin["libpinyin_commit"]}+model20-{pin["model_archive_sha256"]}+dbm-{cell}')
        target = Path(environment.get('CARGO_TARGET_DIR') or
                      str(Path(environment.get('OXPINYIN_TARGET_ROOT') or root / 'target/cells') / cell))
        if not target.is_absolute():
            target = root / target
        env = dict(environment, PINYIN_ORACLE_DBM=cell, ORACLE_DBM=cell,
                   ORACLE_DBM_NAME=BACKEND_FORMATS[feature], CAPI_FEATURE=feature,
                   OXPINYIN_CAPI_BACKEND_EXT=BACKEND_EXTENSIONS[feature], EXPECTED_PIN_REF=expected,
                   CARGO_TARGET_DIR=str(target), CARGO_PROFILE_DEV_OPT_LEVEL='1')
        for key in ('OXPINYIN_CAPI_SO', 'OXPINYIN_ZHUYIN_SO'):
            if env.get(key) and not Path(env[key]).is_absolute():
                env[key] = str(root / env[key])
        return cls(root, cell, feature, BACKEND_EXTENSIONS[feature], BACKEND_FORMATS[feature], expected, target, env)

    def artifact(self, variable: str, filename: str, package: str) -> Path:
        """Return a caller-supplied ABI artifact, or build it directly with Cargo."""
        value = self.environment.get(variable)
        if value:
            artifact = Path(value)
            if not artifact.is_absolute():
                artifact = self.root / artifact
        else:
            command = ['cargo', 'build', '--locked', '--manifest-path', str(self.root / 'Cargo.toml'),
                       '-p', package, *self.feature_args()]
            if slot.run_command(command, env=self.environment, cwd=self.root, stdout=2):
                raise CellError(f'FAIL: building {package}')
            artifact = self.target / 'debug' / filename
        if not artifact.is_file():
            raise CellError(f'FAIL: missing {variable} artifact: {artifact}')
        return artifact
