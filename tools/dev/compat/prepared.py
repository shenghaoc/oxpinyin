"""Prepare selected registry gates' inputs with the existing oracle/packaging recipes."""
import hashlib
from pathlib import Path
import shutil

from ..cell import Settings
from .. import doctor
from ..frontend import frontend_source
from .process import ROOT, PYTHON, run


def require_metadata_tool(environment):
    """Apply the existing cargo-c version floor before any metadata build."""
    tool = next(tool for tool in doctor.TOOLS if tool.name == 'cargo-c')
    if not shutil.which(tool.executable, path=environment.get('PATH')):
        raise doctor.DoctorError('metadata preparation needs cargo-c from the compatibility image')
    print(doctor.check_tool(tool, environment), flush=True)


def prepare(cell, rows, prefix, cache, environment, *, root=ROOT):
    """Return run-all options and set internal metadata paths for selected gates.

    The caller owns the volume lock. All builds use heavy admission; completion
    markers are published only after success, so interrupted builds are retried.
    The image namespace and recipe content separate cached frontend builds.
    No upstream version is repeated here: build-oracle.sh owns both pins.
    """
    root, prefix, cache = Path(root), Path(prefix), Path(cache)
    recipe = root / 'tools/oracle/build-oracle.sh'
    identity = hashlib.sha256(recipe.read_bytes()).hexdigest()
    cache = cache / identity / cell
    cache.mkdir(parents=True, exist_ok=True)
    settings = Settings.read(environment, root)
    model = Path(environment['PINYIN_MODEL_DIR'])
    common = ['bash', str(recipe), '--dbm', cell, '--enable-libzhuyin',
              '--jobs', environment.get('CARGO_BUILD_JOBS', '2')]
    options = []
    names = {row['name'] for row in rows}

    if any('interpolation2.text' in value for row in rows for value in row.get('required_inputs', [])):
        data = cache / 'data'
        # Recopy verified tables rather than modifying the canonical pin/data
        # manifests or trusting tables left by a previous runner.
        shutil.rmtree(data, ignore_errors=True)
        shutil.copytree(prefix / 'lib/libpinyin/data', data)
        shutil.copyfile(model / 'interpolation2.text', data / 'interpolation2.text')
        options += ['--data', f'{cell}={data}']

    if 'import-diff' in names:
        work = cache / 'frontend'
        marker = work / '.complete'
        if not marker.is_file() or frontend_source(work) is None:
            shutil.rmtree(work, ignore_errors=True)
            run([*common, '--work-dir', work, '--prefix', work / 'prefix', '--model-dir', model],
                env=environment, cwd=root, heavy=True)
            if frontend_source(work) is None:
                raise ValueError(f'frontend build did not produce objects and schema: {work}')
            marker.touch()
        options += ['--ibus-build', f'{cell}={work}']

    variants = {row.get('oracle_variant', 'unpatched') for row in rows} - {'unpatched'}
    for variant in sorted(variants):
        configured = settings.oracle(str(prefix), variant)
        patched = Path(configured.environment['PINYIN_ORACLE_PREFIX'])
        expected = configured.environment['EXPECTED_ORACLE_PIN_REF']
        patch_dir = root / 'tools/bisection/patches' / variant
        key = environment['OXPINYIN_COMPAT_IMAGE_ID'] + identity + expected
        marker = patched / '.compat-complete'
        if (not marker.is_file() or marker.read_text() != key or
                not (patched / 'lib/libpinyin.so').is_file()):
            shutil.rmtree(patched, ignore_errors=True)
            run([*common, '--libs-only', '--apply-patches', patch_dir,
                 '--work-dir', cache / variant, '--prefix', patched],
                env=environment, cwd=root, heavy=True)
            if expected not in (patched / 'oracle-pin.txt').read_text().splitlines():
                raise ValueError(f'patched oracle identity mismatch: {patched}')
            # This instrumentation changes export joining only; reuse the
            # verified unpatched system tables, never regenerate different data.
            shutil.copytree(prefix / 'lib/libpinyin/data', patched / 'lib/libpinyin/data')
            marker.write_text(key)

    if 'pc-metadata' in names:
        require_metadata_tool(environment)
        stage = Path(environment['OXPINYIN_TARGET_ROOT']) / cell / 'metadata'
        pin_root, subject_root = stage / 'pin', stage / 'subject'
        for directory in (pin_root, subject_root):
            shutil.rmtree(directory, ignore_errors=True)
        shutil.copytree(prefix, pin_root / prefix.relative_to('/'))
        metadata_env = dict(environment, CARGO_TARGET_DIR=str(stage / 'target'),
                            CARGO_PROFILE_DEV_OPT_LEVEL='1')
        for library in ('libpinyin', 'libzhuyin'):
            run([*PYTHON, 'packaging', 'install', library, f'--prefix={prefix}',
                 f'--destdir={subject_root}', '--', '--profile', 'dev', '--locked',
                 '--no-default-features', '--features',
                 settings.feature + (',shipped' if library == 'libpinyin' else '')],
                env=metadata_env, cwd=root, heavy=True)
        environment.update(OXPINYIN_METADATA_PIN_ROOT=str(pin_root),
                           OXPINYIN_METADATA_SUBJECT_ROOT=str(subject_root),
                           OXPINYIN_METADATA_PREFIX=str(prefix))
    return options
