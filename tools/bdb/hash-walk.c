/* hash-walk.c — walks a libpinyin bigram.db with libdb directly and checks
 * every SingleGram invariant the Rust backend relies on.
 *
 * Ground truth with no Rust involved: this is what established the layout
 * in docs/findings/berkeleydb-backend.md, and under
 * -fsanitize=address,undefined it is the half of the sanitizer gate that
 * has C to instrument (misaligned DBT loads, out-of-bounds chunk indexing,
 * overflow in the (size - 4) / 8 item count).
 */
#include <db.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

int main(int argc, char **argv) {
    if (argc < 2) { fprintf(stderr, "usage: probe FILE\n"); return 2; }
    DB *db = NULL;
    int rc = db_create(&db, NULL, 0);
    if (rc != 0) { fprintf(stderr, "db_create: %s\n", db_strerror(rc)); return 1; }
    rc = db->open(db, NULL, argv[1], NULL, DB_HASH, DB_RDONLY, 0644);
    if (rc != 0) { fprintf(stderr, "open: %s\n", db_strerror(rc)); return 1; }

    DBC *cur = NULL;
    rc = db->cursor(db, NULL, &cur, 0);
    if (rc != 0) { fprintf(stderr, "cursor: %s\n", db_strerror(rc)); return 1; }

    DBT key, data;
    memset(&key, 0, sizeof key); memset(&data, 0, sizeof data);
    long records = 0, items_total = 0, bad_size = 0, unsorted = 0, total_mismatch = 0;
    long zero_item_nonzero_total = 0, keysize_bad = 0;
    int shown = 0;
    while ((rc = cur->c_get(cur, &key, &data, DB_NEXT)) == 0) {
        records++;
        if (key.size != 4) { keysize_bad++; continue; }
        uint32_t prev; memcpy(&prev, key.data, 4);
        if (data.size < 4 || ((data.size - 4) % 8) != 0) { bad_size++; continue; }
        uint32_t total; memcpy(&total, (char *)data.data + 0, 4);
        uint32_t n = (data.size - 4) / 8;
        items_total += n;
        uint32_t sum = 0, last_tok = 0;
        for (uint32_t i = 0; i < n; i++) {
            uint32_t tok, freq;
            memcpy(&tok,  (char *)data.data + 4 + i * 8,     4);
            memcpy(&freq, (char *)data.data + 4 + i * 8 + 4, 4);
            if (i > 0 && tok <= last_tok) unsorted++;
            last_tok = tok;
            sum += freq;
        }
        if (n == 0 && total != 0) zero_item_nonzero_total++;
        if (sum != total) total_mismatch++;
        if (shown < 3) {
            printf("record %ld: key=4 bytes prev=0x%08x  value=%u bytes total_freq=%u items=%u\n",
                   records, prev, (unsigned)data.size, total, n);
            for (uint32_t i = 0; i < n && i < 4; i++) {
                uint32_t tok, freq;
                memcpy(&tok,  (char *)data.data + 4 + i * 8,     4);
                memcpy(&freq, (char *)data.data + 4 + i * 8 + 4, 4);
                printf("    item[%u] token=0x%08x freq=%u\n", i, tok, freq);
            }
            shown++;
        }
    }
    cur->c_close(cur);
    db->close(db, 0);
    printf("\n--- LAYOUT VERDICT over the real system file ---\n");
    printf("records                       %ld\n", records);
    printf("successor items               %ld\n", items_total);
    printf("keys not 4 bytes              %ld\n", keysize_bad);
    printf("values failing 4 + 8n         %ld\n", bad_size);
    printf("item arrays not ascending     %ld\n", unsorted);
    printf("total_freq != sum(item freq)  %ld\n", total_mismatch);
    printf("zero items but nonzero total  %ld\n", zero_item_nonzero_total);
    return 0;
}
