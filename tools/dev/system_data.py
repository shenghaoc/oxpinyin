"""Resolve and validate one cell's native system data, without shell globals."""
from dataclasses import dataclass
from pathlib import Path
import re
import shutil
import sys

from .cell import CellError, Settings

NAMES = {'db': 'Berkeley DB', 'kct': 'Kyoto Cabinet', 'tkt': 'tkrzw'}
FEATURES = {'db': 'bdb', 'kct': 'kyotocabinet', 'tkt': 'tkrzw'}
CORE = ('pinyin_index', 'phrase_index', 'bigram')
NATIVE = ('pinyin_index.bin', 'phrase_index.bin', 'bigram.db')


def data_extension(directory: Path) -> str | None:
    manifest, config = directory / 'datagen-manifest.txt', directory / 'table.conf'
    value = ''
    if manifest.is_file():
        match = re.search(r'^backend=(.*)$', manifest.read_text(), re.M)
        value = match[1] if match else ''
    if not value and config.is_file():
        match = re.search(r'^database format:\s*([^\n]*)', config.read_text(), re.M)
        value = {'BerkeleyDB': 'db', 'KyotoCabinet': 'kct', 'Tkrzw': 'tkt'}.get(
            ''.join(match[1].split()) if match else '', '')
    return value if value in NAMES else None


@dataclass(frozen=True)
class SystemData:
    settings: Settings
    cwd: Path | None = None

    def require_backend(self, directory: Path, label: str) -> None:
        have, want = data_extension(directory), self.settings.extension
        if have is None:
            raise CellError(f'fatal: {label}: cannot tell which backend wrote\n  {directory}\n'
                            '  (no datagen-manifest.txt backend= line and no known\n'
                            '  table.conf "database format:" token)', 3)
        if have != want:
            raise CellError(f'fatal: {label}: BACKEND MISMATCH\n  data directory {directory}\n'
                            f'  was written for {NAMES[have]}, but the library under test is {NAMES[want]}.\n'
                            f'  Point at a {NAMES[want]} data directory built for that backend.', 3)

    def detect(self, directory: Path) -> str | None:
        ext = self.settings.extension
        peer = all((directory / f'{stem}.{ext}').is_file() for stem in CORE)
        if not peer and not all((directory / name).is_file() for name in NATIVE):
            return None
        self.require_backend(directory, 'system data directory')
        return ext if peer else 'bin'

    def require_complete(self, directory: Path, variable: str, label: str) -> None:
        missing = [name for name in ('gb_char.bin', 'table.conf') if not (directory / name).is_file()]
        found = next((name for name in ('pinyin_index.db', 'pinyin_index.bin', 'pinyin_index.kct', 'pinyin_index.tkt')
                      if (directory / name).is_file()), None)
        if found is None:
            missing.append('pinyin_index.{bin,kct,tkt,db}')
        else:
            ext = found.rsplit('.', 1)[1]
            siblings = ('phrase_index.bin', 'bigram.db') if ext == 'bin' else (f'phrase_index.{ext}', f'bigram.{ext}')
            missing.extend(name for name in siblings if not (directory / name).is_file())
        if not missing:
            self.require_backend(directory, label)
            return
        raise CellError(f'fatal: {label}: the system directory is incomplete.\n  {directory}\n'
                        f'  (from ${variable}, $OXPINYIN_SYSTEM_DIR, or a conventional location)\n'
                        '\nMissing:\n' + ''.join('  ' + name + '\n' for name in missing) +
                        '\nA system data directory is the output of libpinyin\'s own\n'
                        'gen_binary_files / import_interpolation / gen_unigram sequence\n'
                        "(oxpinyin-utils' pinned binaries), or a libpinyin install's data/\n"
                        'on Kyoto Cabinet, tkrzw or Berkeley DB:\n'
                        "the chunk files, table.conf, and the DBMs under the backend's names.", 3)

    def resolve(self, variable: str, label: str) -> Path:
        settings, ext = self.settings, self.settings.extension
        value = settings.environment.get(variable) or settings.environment.get('OXPINYIN_SYSTEM_DIR')
        if not value:
            conventional = settings.root / 'target/datagen' / ext
            legacy = Path('/tmp/oxpinyin-export')
            if (conventional / 'gb_char.bin').is_file():
                value = str(conventional)
            elif (legacy / 'gb_char.bin').is_file() and data_extension(legacy) == ext:
                value = str(legacy)
        if value:
            directory = Path(value)
            if not directory.is_absolute() and self.cwd is not None:
                directory = self.cwd / directory
            self.require_complete(directory, variable, label)
            return directory
        if settings.environment.get('OXPINYIN_ALLOW_MINI_FIXTURE') == '1':
            print('\n################################################################\n'
                  f'# {label} is running against fixtures/w3 -- the MINI tables.\n'
                  '# This is NOT a parity measurement. The mini tables hold a\n'
                  '# few hundred phrases, so a divergence here says nothing about\n'
                  '# agreement with the pin. Unset OXPINYIN_ALLOW_MINI_FIXTURE to\n'
                  '# make this fatal again.\n'
                  '################################################################\n', file=sys.stderr)
            directory = settings.root / 'fixtures/w3' / ext
            self.require_backend(directory, label)
            return directory
        raise CellError(f'fatal: {label} has an oracle but no {NAMES[ext]} system data directory.\n\n'
                        'Looked at, in order:\n'
                        f"  ${variable}          (this runner's own variable)\n"
                        '  $OXPINYIN_SYSTEM_DIR   (set once for a whole sweep)\n'
                        f'  {settings.root}/target/datagen/{ext}\n'
                        '  /tmp/oxpinyin-export   (only if written by the same backend)\n\n'
                        "Another backend's directory is never used instead: the three\n"
                        'DBMs share file names, so it would load or score the wrong data.\n\n'
                        'Build one from the pinned model with libpinyin\'s own sequence\n'
                        '(its data/Makefile.am:60-62); table.conf is the pin\'s configured\n'
                        'data/table.conf, whose committed rendering is fixtures/w3/<ext>:\n'
                        '  just fetch-model\n'
                        f'  ROOT=$PWD; MODEL=$ROOT/target/model20/extracted; OUT=$ROOT/target/datagen/{ext}\n'
                        '  mkdir -p "$OUT" && cd "$OUT"\n'
                        f'  cp "$ROOT/fixtures/w3/{ext}/table.conf" .\n'
                        f'  cargo run --release --manifest-path "$ROOT/Cargo.toml" \\\n'
                        f'    -p oxpinyin-utils --no-default-features --features {FEATURES[ext]} \\\n'
                        '    --bin gen_binary_files -- --gen-punct-table --table-dir "$MODEL"\n'
                        f'  cargo run --release --manifest-path "$ROOT/Cargo.toml" \\\n'
                        f'    -p oxpinyin-utils --no-default-features --features {FEATURES[ext]} \\\n'
                        '    --bin import_interpolation -- --table-dir "$MODEL" < "$MODEL/interpolation2.text"\n'
                        f'  cargo run --release --manifest-path "$ROOT/Cargo.toml" \\\n'
                        f'    -p oxpinyin-utils --no-default-features --features {FEATURES[ext]} \\\n'
                        '    --bin gen_unigram -- --table-dir "$MODEL"\n\n'
                        'Refusing rather than falling back to fixtures/w3: scoring a real\n'
                        'oracle against the mini tables reports DIVERGENCE that means\n'
                        'nothing. Set OXPINYIN_ALLOW_MINI_FIXTURE=1 if you deliberately\n'
                        'want the mini-fixture run.', 3)

    def copy(self, source: Path, destination: Path) -> None:
        ext = self.detect(source)
        if ext is None:
            raise CellError(f'fatal: {source} holds no complete core-table set in one extension', 3)
        if ext == 'bin':
            # The native shell contract used find -type f without following links.
            files = [file for file in source.iterdir()
                     if file.is_file() and not file.is_symlink() and file.name != 'interpolation2.text']
        else:
            files = [source / f'{stem}.{ext}' for stem in CORE]
            files += [source / f'punct.{ext}', *sorted(source.glob(f'addon_*_pinyin_index.{ext}')),
                      *sorted(source.glob(f'addon_*_phrase_index.{ext}'))]
        for file in files:
            if file.is_file():
                shutil.copyfile(file, destination / file.name)
