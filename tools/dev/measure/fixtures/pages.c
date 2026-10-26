/* Source of the tools/dev/measure/pages.test.py object fixtures; built by
 * build-pages.py. Three functions, a constant table, a writable word and a
 * thread-local word give code, read-only, data and TLS symbols. */
int pages_counter = 3;
const unsigned char pages_table[96] = {1, 2, 3};
__thread int pages_tls = 7;

int pages_first(int x) { return x * 3 + pages_counter; }
int pages_second(int x) { return pages_first(x) ^ 0x55; }
int pages_third(int x) { return pages_table[x & 63] + pages_second(x) + pages_tls; }
