"""The pinned frontend objects consumed by the classic import/export harness."""

# src/Makefile.am at build-oracle.sh's frontend pin lists these sources.
# Its recipe disables Lua, cloud and English input and retains table input.
# frontend-import.cc replaces PYMain.o; the remaining build objects are required
# together, including dependencies of PYLibPinyin.o and PYPConfig.o.
FRONTEND_OBJECTS = tuple(f'ibus_engine_libpinyin-{stem}.o' for stem in (
    'PYConfig', 'PYEditor', 'PYEngine', 'PYFallbackEditor', 'PYHalfFullConverter',
    'PYLibPinyin', 'PYPBopomofoEditor', 'PYPBopomofoEngine', 'PYPConfig',
    'PYPDoublePinyinEditor', 'PYPEmojiCandidates', 'PYPFullPinyinEditor',
    'PYPLibPinyinCandidates', 'PYPPhoneticEditor', 'PYPPinyinEditor',
    'PYPPinyinEngine', 'PYPSuggestionCandidates', 'PYPSuggestionEditor',
    'PYPTradCandidates', 'PYPinyinProperties', 'PYPunctEditor',
    'PYSimpTradConverter', 'PYTableDatabase', 'PYTableEditor', 'PYXMLUtil',
))


def frontend_source(work):
    """Return a source tree only when every required object and schema exists."""
    return next((source for source in sorted((work / 'src').glob('ibus-libpinyin-*'))
                 if all((source / 'src' / name).is_file() for name in FRONTEND_OBJECTS)
                 and (source / 'data/com.github.libpinyin.ibus-libpinyin.gschema.xml').is_file()), None)
