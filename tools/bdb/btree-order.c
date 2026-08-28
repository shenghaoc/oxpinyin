/* btree-order.c — confirms DB_BTREE's DEFAULT comparator order over the
 * raw little-endian u32 array keys libpinyin uses, and gives the
 * sanitizers a create/put/cursor path to instrument.
 *
 * Opened exactly as libpinyin opens its own B-trees: NULL environment,
 * NULL transaction, and NO set_bt_compare. The walk this prints is the
 * evidence behind docs/findings/berkeleydb-backend.md's ordering claim —
 * raw-byte order, which is neither integer order nor big-endian order.
 */
#include <db.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static const uint32_t ONE[] = {
    0x00000200u, 0x000000FFu, 0x07000100u, 0x00000100u,
    0x00000001u, 0x0000FFFFu, 0x000001FFu, 0x07000001u,
    0x00010000u, 0x070000FFu, 0x00FF0000u, 0x01000000u,
};

int main(int argc, char **argv) {
    const char *path = argc > 1 ? argv[1] : "btree-order.db";
    unlink(path);
    DB *db = NULL;
    int rc = db_create(&db, NULL, 0);
    if (rc) { fprintf(stderr, "db_create: %s\n", db_strerror(rc)); return 1; }
    /* Exactly libpinyin's open: NULL env, NULL txn, no set_bt_compare. */
    rc = db->open(db, NULL, path, NULL, DB_BTREE, DB_CREATE, 0644);
    if (rc) { fprintf(stderr, "open: %s\n", db_strerror(rc)); return 1; }

    size_t n = sizeof ONE / sizeof ONE[0];
    for (size_t i = 0; i < n; i++) {
        DBT k, d; memset(&k, 0, sizeof k); memset(&d, 0, sizeof d);
        uint32_t one = ONE[i];
        k.data = &one; k.size = sizeof one;      /* 1-element key */
        d.data = (void *)"v"; d.size = 1;
        rc = db->put(db, NULL, &k, &d, 0);
        if (rc) { fprintf(stderr, "put: %s\n", db_strerror(rc)); return 1; }
        /* a 2-element key sharing the same first element: tests "then length" */
        uint32_t two[2] = { one, 0x00000102u };
        DBT k2; memset(&k2, 0, sizeof k2);
        k2.data = two; k2.size = sizeof two;
        rc = db->put(db, NULL, &k2, &d, 0);
        if (rc) { fprintf(stderr, "put2: %s\n", db_strerror(rc)); return 1; }
    }

    DBC *cur = NULL;
    if ((rc = db->cursor(db, NULL, &cur, 0))) { fprintf(stderr, "cursor: %s\n", db_strerror(rc)); return 1; }
    DBT k, d; memset(&k, 0, sizeof k); memset(&d, 0, sizeof d);
    printf("%-4s %-26s %s\n", "#", "key bytes", "decoded LE u32 elements");
    int i = 0;
    while (cur->c_get(cur, &k, &d, DB_NEXT) == 0) {
        printf("%-4d ", ++i);
        unsigned char *p = k.data;
        char buf[64] = {0}; size_t off = 0;
        for (unsigned j = 0; j < k.size; j++)
            off += (size_t)snprintf(buf + off, sizeof buf - off, "%02x", p[j]);
        printf("%-26s ", buf);
        for (unsigned j = 0; j + 4 <= k.size; j += 4) {
            uint32_t v; memcpy(&v, p + j, 4);
            printf("0x%08x ", v);
        }
        printf("\n");
    }
    cur->c_close(cur);
    db->close(db, 0);
    return 0;
}
