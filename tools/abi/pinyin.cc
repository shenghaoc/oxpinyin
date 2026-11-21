#include <pinyin.h>

static_assert(sizeof(PinyinKey *) == sizeof(void *));
static_assert(sizeof(PinyinKeyPos *) == sizeof(void *));

int main() {
    auto volatile getter = &pinyin_get_parsed_input_length;
    return getter == nullptr;
}
