"""Shared fixtures: repository layout, fixture data, the native transcript."""

import json
import subprocess
from pathlib import Path

import pytest

CRATE_DIR = Path(__file__).resolve().parents[1]
REPO_ROOT = CRATE_DIR.parents[1]


@pytest.fixture(scope="session")
def repo_root() -> Path:
    return REPO_ROOT


@pytest.fixture(scope="session")
def crate_dir() -> Path:
    return CRATE_DIR


@pytest.fixture(scope="session")
def fixture_w3(repo_root: Path) -> Path:
    """The committed mini system-data fixture the Rust tests use too.

    The wheel is built with the crate's default backend (Kyoto Cabinet:
    the CI job installs its C library for exactly that), so the fixture
    directory is the one datagen wrote in libpinyin's own layout.
    """
    path = repo_root / "fixtures" / "w3" / "kct"
    assert (path / "pinyin_index.bin").is_file(), "w3 fixture missing"
    return path


@pytest.fixture(scope="session")
def parity_corpus(crate_dir: Path) -> dict:
    return json.loads((crate_dir / "parity-corpus.json").read_text())


@pytest.fixture(scope="session")
def native_transcript(tmp_path_factory, repo_root: Path, crate_dir: Path) -> dict:
    """Regenerates the native-side transcript through the pure-Rust API.

    ``native-dump`` links the same runtime module the binding wraps, with no
    Python anywhere in the process — so byte-comparing its output against the
    binding's replay proves the two surfaces compute identically.
    """
    out_dir = tmp_path_factory.mktemp("native-dump")
    out_path = out_dir / "native.json"
    subprocess.run(
        [
            "cargo",
            "run",
            "--quiet",
            "-p",
            "oxpinyin-python",
            "--bin",
            "native-dump",
            "--",
            str(crate_dir / "parity-corpus.json"),
            str(repo_root / "fixtures" / "w3" / "kct"),
            str(out_path),
        ],
        check=True,
        cwd=repo_root,
    )
    return json.loads(out_path.read_text(encoding="utf-8"))


@pytest.fixture
def make_engine(fixture_w3: Path):
    """Opens engines in fixture mode; defaults to no user directory."""
    import oxpinyin

    created = []

    def _make(user_dir=None):
        engine = oxpinyin.Engine.from_fixture_dir(
            str(fixture_w3),
            None if user_dir is None else str(user_dir),
        )
        created.append(engine)
        return engine

    yield _make
    del created
