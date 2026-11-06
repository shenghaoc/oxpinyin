#include <stdio.h>
#include "shared.h"

void only_pinyin() {
    printf("pinyin\n");
}

void only_tool() {
    *(char **) NULL = g_strdup("tool");
}

void kept_by_nothing() {
    fprintf(stderr, "dead\n");
}
