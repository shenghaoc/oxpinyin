/* bdb-to-kc.c — transcribe a libpinyin bigram.db from Berkeley DB into
 * Kyoto Cabinet, copying every key and value byte for byte.
 *
 * Why this exists. A Kyoto-Cabinet-built libpinyin is what this backend
 * is for, but not every machine has one installed; this one has a
 * Berkeley-DB-built libpinyin (its bigram.db carries DB_HASHMAGIC). The
 * SingleGram chunk and the four-byte native-endian key are
 * backend-independent -- ngram.cpp is unconditional in
 * src/storage/Makefile.am:72 while ngram_bdb.cpp and ngram_kyotodb.cpp
 * are added under `if BERKELEYDB` / `if KYOTOCABINET` -- so the same
 * records live in both formats under the same keys.
 *
 * This tool makes that concrete: real libpinyin data, moved into the
 * physical container the Kyoto Cabinet backend reads, with no Rust
 * anywhere in the path. What it does NOT do is prove that a
 * Kyoto-Cabinet-built libpinyin would have written the same file; only a
 * machine with one installed can show that.
 *
 * Note the #type=kch on the output path: libpinyin names its file
 * bigram.db whatever DBM it was built against, and Kyoto Cabinet's
 * PolyDB (which the C API is) fails on an unrecognised suffix. The
 * tuning parameter is how a file called bigram.db is opened as a hash
 * database at all.
 *
 * Usage: bdb-to-kc SOURCE-BDB DEST-KC
 * Exit: 0 on success; 1 on failure; 2 on usage error.
 */
#include <db.h>
#include <kclangc.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

/* A failed transcription must not leave a half-written destination that
 * looks like data: close the handle, release it, and remove the partial
 * file. Only failure paths after the destination was opened call this;
 * the success path keeps its explicit close, count, and release. */
static void drop_destination(KCDB *dst, const char *dest) {
    kcdbclose(dst);
    kcdbdel(dst);
    remove(dest);
}

int main(int argc, char **argv) {
    if (argc != 3) {
        fprintf(stderr, "usage: bdb-to-kc SOURCE-BDB DEST-KC\n");
        return 2;
    }

    DB *src = NULL;
    int rc = db_create(&src, NULL, 0);
    if (rc != 0 || src == NULL) {
        fprintf(stderr, "db_create: %s\n", db_strerror(rc));
        return 1;
    }
    rc = src->open(src, NULL, argv[1], NULL, DB_HASH, DB_RDONLY, 0644);
    if (rc != 0) {
        fprintf(stderr, "open %s: %s\n", argv[1], db_strerror(rc));
        return 1;
    }

    /* Refuse SOURCE == DEST before the remove() below would unlink the
     * very file the cursor is about to read: textual equality, and the
     * two paths naming one inode when the destination already exists
     * (a symlink or a different spelling counts too). */
    if (strcmp(argv[1], argv[2]) == 0) {
        fprintf(stderr, "SOURCE and DEST are the same path: %s\n", argv[1]);
        return 1;
    }
    struct stat sst, dst_st;
    if (stat(argv[1], &sst) == 0 && stat(argv[2], &dst_st) == 0
        && sst.st_dev == dst_st.st_dev && sst.st_ino == dst_st.st_ino) {
        fprintf(stderr, "SOURCE and DEST name the same file: %s, %s\n",
                argv[1], argv[2]);
        return 1;
    }

    /* PolyDB picks the class from the suffix and fails on an unknown one,
     * so the #type= override is mandatory for libpinyin's filename. */
    size_t spec_len = strlen(argv[2]) + sizeof("#type=kch");
    char *spec = malloc(spec_len);
    if (!spec) { fprintf(stderr, "out of memory\n"); return 1; }
    snprintf(spec, spec_len, "%s#type=kch", argv[2]);

    remove(argv[2]);
    KCDB *dst = kcdbnew();
    if (!dst) { fprintf(stderr, "kcdbnew failed\n"); free(spec); return 1; }
    if (!kcdbopen(dst, spec, KCOWRITER | KCOCREATE)) {
        fprintf(stderr, "kcdbopen %s: %s\n", spec, kcecodename(kcdbecode(dst)));
        kcdbdel(dst);
        free(spec);
        return 1;
    }

    DBC *cur = NULL;
    rc = src->cursor(src, NULL, &cur, 0);
    if (rc != 0 || cur == NULL) {
        fprintf(stderr, "cursor: %s\n", db_strerror(rc));
        drop_destination(dst, argv[2]);
        free(spec);
        return 1;
    }

    DBT key, data;
    memset(&key, 0, sizeof key);
    memset(&data, 0, sizeof data);
    long records = 0, items = 0;
    while ((rc = cur->c_get(cur, &key, &data, DB_NEXT)) == 0) {
        if (key.size != 4 || data.size < 4 || ((data.size - 4) % 8) != 0) {
            fprintf(stderr, "record %ld has a shape no SingleGram has "
                            "(key %u bytes, value %u bytes)\n",
                    records, (unsigned)key.size, (unsigned)data.size);
            drop_destination(dst, argv[2]);
            free(spec);
            return 1;
        }
        if (!kcdbset(dst, (const char *)key.data, key.size,
                     (const char *)data.data, data.size)) {
            fprintf(stderr, "kcdbset: %s\n", kcecodename(kcdbecode(dst)));
            drop_destination(dst, argv[2]);
            free(spec);
            return 1;
        }
        records++;
        items += (data.size - 4) / 8;
    }
    if (rc != DB_NOTFOUND) {
        fprintf(stderr, "cursor walk: %s\n", db_strerror(rc));
        drop_destination(dst, argv[2]);
        free(spec);
        return 1;
    }

    cur->c_close(cur);
    src->close(src, 0);
    if (!kcdbsync(dst, 1, NULL, NULL)) {
        fprintf(stderr, "kcdbsync: %s\n", kcecodename(kcdbecode(dst)));
        drop_destination(dst, argv[2]);
        free(spec);
        return 1;
    }
    int64_t count = kcdbcount(dst);
    kcdbclose(dst);
    kcdbdel(dst);
    free(spec);

    printf("transcribed %ld records (%ld successor items) into %s\n",
           records, items, argv[2]);
    if (count != records) {
        fprintf(stderr, "destination holds %lld records, source had %ld\n",
                (long long)count, records);
        return 1;
    }
    return 0;
}
