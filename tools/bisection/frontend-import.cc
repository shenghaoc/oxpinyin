/*
 * frontend-import.cc — drive the pinned frontend's real import/export path.
 *
 * Links the object files from tools/oracle/build-oracle.sh's ibus-libpinyin
 * build (same pin, tag 1.16.5) minus PYMain.o, then calls the actual
 * LibPinyinBackEnd::initPinyinContext / importPinyinDictionary /
 * exportPinyinDictionary methods (PYLibPinyin.cc:230-353).
 *
 * Usage:
 *   frontend-import <input-file> <output-file>
 *
 * Exit status: 0 when importPinyinDictionary and exportPinyinDictionary
 * both return TRUE. The exported file is the frontend's own classic-format
 * output, which the caller diffs against oxpinyin-dictool.
 */
#include <cstdio>

#include "PYLibPinyin.h"
#include "PYPConfig.h"

int main(int argc, char **argv) {
    if (argc != 3) {
        std::fprintf(stderr, "Usage: %s <input-file> <output-file>\n", argv[0]);
        return 2;
    }

    PY::LibPinyinBackEnd::init();
    PY::PinyinConfig::init();
    PY::LibPinyinBackEnd &backend = PY::LibPinyinBackEnd::instance();

    /* importPinyinDictionary does not create the context itself; the real
     * frontend allocates an instance first, which lazily runs
     * initPinyinContext (PYLibPinyin.cc:98-105). */
    pinyin_instance_t *instance = backend.allocPinyinInstance();
    if (!instance) {
        std::fprintf(stderr, "pinyin_alloc_instance failed\n");
        return 3;
    }

    const bool imported = backend.importPinyinDictionary(argv[1]);
    backend.freePinyinInstance(instance);
    if (!imported) {
        std::fprintf(stderr, "importPinyinDictionary failed\n");
        return 4;
    }

    const bool exported = backend.exportPinyinDictionary(argv[2]);
    return exported ? 0 : 5;
}
