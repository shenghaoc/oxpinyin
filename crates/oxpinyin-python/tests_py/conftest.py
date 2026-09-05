"""Shared fixtures: repository layout, fixture data, the native transcript."""

import json
import subprocess
import sys
import sysconfig
from pathlib import Path

import pytest

CRATE_DIR = Path(__file__).resolve().parents[1]
REPO_ROOT = CRATE_DIR.parents[1]

#: Mirror of the parity-runner input cap in `dump.rs::effective_input`:
#: replay inputs resolve identically on both sides, and an adversarial
#: corpus file must not OOM the pytest process any more than it may crash
#: the dump.
INPUT_CAP = 1 << 20


def resolve_input(case: dict) -> str:
    """Resolves a case's effective input: `input`, or `repeat.unit` × times."""
    if "input" in case:
        return case["input"]
    repeat = case.get("repeat") or {}
    unit, times = repeat.get("unit", ""), int(repeat.get("times", 0))
    # Byte length, not character count: the Rust side measures
    # `str::len` (UTF-8 bytes), so a non-ASCII unit must be encoded
    # before measuring, or the two drivers disagree on the cap.
    total = len(unit.encode("utf-8")) * times
    if total > INPUT_CAP:
        raise ValueError(
            f"repeated input is {total} bytes, past the {INPUT_CAP}-byte corpus cap"
        )
    return unit * times


@pytest.fixture(scope="session", autouse=True)
def extension_imports_keep_the_gil_as_found():
    """Both extension modules import, and a free-threaded build stays free.

    Importing an extension that does not declare `Py_MOD_GIL_NOT_USED`
    re-enables the GIL process-wide on 3.13+, which would leave
    `test_shared_engine_is_thread_safe` vacuous while the suite stayed
    green. The import runs on every interpreter — a module that fails to
    load is caught everywhere — and the GIL assert is gated on the
    interpreter actually being a free-threaded build.

    That gate is the whole point of `Py_GIL_DISABLED`, not a way around a
    failure: only a build whose GIL is off has one to re-enable. CI runs
    Debian's stock `python3` (GIL-enabled: `actions/setup-python` cannot
    serve a container job, so the free-threaded selector went with it),
    where the interpreter's own default would fail an ungated assert while
    proving nothing. On a free-threaded build the check bites exactly as
    before. (`sys._is_gil_enabled` only exists from 3.13.)
    """
    import oxpinyin
    import oxpinyin.zhuyin

    assert oxpinyin is not None and oxpinyin.zhuyin is not None
    is_gil_enabled = getattr(sys, "_is_gil_enabled", None)
    if sysconfig.get_config_var("Py_GIL_DISABLED") and is_gil_enabled is not None:
        assert not is_gil_enabled(), "extension import re-enabled the GIL"


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
def zhuyin_parity_corpus(crate_dir: Path) -> dict:
    return json.loads((crate_dir / "parity-corpus-zhuyin.json").read_text())


@pytest.fixture(scope="session")
def zhuyin_native_transcript(tmp_path_factory, repo_root: Path, crate_dir: Path) -> dict:
    """Regenerates the zhuyin native-side transcript through the pure-Rust session.

    ``zhuyin-dump`` drives the same [`oxpinyin_facade`] orchestration the
    binding wraps, with no Python anywhere in the process — so comparing
    its output against the binding's replay proves the two surfaces
    compute identically.
    """
    out_dir = tmp_path_factory.mktemp("zhuyin-dump")
    out_path = out_dir / "zhuyin-native.json"
    subprocess.run(
        [
            "cargo",
            "run",
            "--quiet",
            "-p",
            "oxpinyin-python",
            "--bin",
            "zhuyin-dump",
            "--",
            str(crate_dir / "parity-corpus-zhuyin.json"),
            str(repo_root / "fixtures" / "w3" / "kct"),
            str(out_path),
        ],
        check=True,
        cwd=repo_root,
    )
    return json.loads(out_path.read_text(encoding="utf-8"))


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


@pytest.fixture
def make_zhuyin_engine(fixture_w3: Path):
    """Opens zhuyin engines in fixture mode; defaults to no user directory."""
    import oxpinyin.zhuyin

    created = []

    def _make(user_dir=None):
        engine = oxpinyin.zhuyin.Engine.from_fixture_dir(
            str(fixture_w3),
            None if user_dir is None else str(user_dir),
        )
        created.append(engine)
        return engine

    yield _make
    del created
