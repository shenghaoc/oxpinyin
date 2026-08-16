// cpp-smoke.cc — C++ translation-unit gate for crates/oxpinyin-capi/pinyin.h.
//
// The fork consumes pinyin.h from C++ `.cc` files; this TU is the minimal
// regression for that class of build breakage. It includes the generated
// header, compiles as C++ (so the `extern "C"` guard and the fork-referenced
// typedefs/constants must be valid C++), links `libpinyin_capi.so`, and
// exercises the W8 gap symbol with its pinned semantics.
//
// Build/run: tools/bisection/run-cpp-smoke.sh.

#include <pinyin.h>

#include <cstdlib>
#include <cstring>

namespace {

struct Session {
    pinyin_context_t *context = nullptr;
    pinyin_instance_t *instance = nullptr;

    Session() = default;
    Session(const Session &) = delete;
    Session &operator=(const Session &) = delete;

    ~Session() {
        if (instance != nullptr) {
            pinyin_free_instance(instance);
        }
        if (context != nullptr) {
            pinyin_fini(context);
        }
    }
};

}  // namespace

int main(int argc, char **argv) {
    if (argc != 3) {
        return 64;
    }

    // Compile-time checks for the fork-referenced non-function surface.
    static_assert(PINYIN_INCOMPLETE == (1U << 3), "PINYIN_INCOMPLETE");
    static_assert(ZHUYIN_INCOMPLETE == (1U << 4), "ZHUYIN_INCOMPLETE");
    static_assert(USE_TONE == (1U << 5), "USE_TONE");
    static_assert(USE_DIVIDED_TABLE == (1U << 7), "USE_DIVIDED_TABLE");
    static_assert(USE_RESPLIT_TABLE == (1U << 8), "USE_RESPLIT_TABLE");
    static_assert(DYNAMIC_ADJUST == (1U << 9), "DYNAMIC_ADJUST");
    static_assert(PINYIN_AMB_C_CH == (1U << 10), "PINYIN_AMB_C_CH");
    static_assert(PINYIN_AMB_S_SH == (1U << 11), "PINYIN_AMB_S_SH");
    static_assert(PINYIN_AMB_Z_ZH == (1U << 12), "PINYIN_AMB_Z_ZH");
    static_assert(PINYIN_AMB_F_H == (1U << 13), "PINYIN_AMB_F_H");
    static_assert(PINYIN_AMB_G_K == (1U << 14), "PINYIN_AMB_G_K");
    static_assert(PINYIN_AMB_L_N == (1U << 15), "PINYIN_AMB_L_N");
    static_assert(PINYIN_AMB_L_R == (1U << 16), "PINYIN_AMB_L_R");
    static_assert(PINYIN_AMB_AN_ANG == (1U << 17), "PINYIN_AMB_AN_ANG");
    static_assert(PINYIN_AMB_EN_ENG == (1U << 18), "PINYIN_AMB_EN_ENG");
    static_assert(PINYIN_AMB_IN_ING == (1U << 19), "PINYIN_AMB_IN_ING");
    static_assert(PINYIN_AMB_ALL == (0x3ffU << 10), "PINYIN_AMB_ALL");
    static_assert(PINYIN_CORRECT_GN_NG == (1U << 21), "PINYIN_CORRECT_GN_NG");
    static_assert(PINYIN_CORRECT_MG_NG == (1U << 22), "PINYIN_CORRECT_MG_NG");
    static_assert(PINYIN_CORRECT_IOU_IU == (1U << 23), "PINYIN_CORRECT_IOU_IU");
    static_assert(PINYIN_CORRECT_UEI_UI == (1U << 24), "PINYIN_CORRECT_UEI_UI");
    static_assert(PINYIN_CORRECT_UEN_UN == (1U << 25), "PINYIN_CORRECT_UEN_UN");
    static_assert(PINYIN_CORRECT_UE_VE == (1U << 26), "PINYIN_CORRECT_UE_VE");
    static_assert(PINYIN_CORRECT_V_U == (1U << 27), "PINYIN_CORRECT_V_U");
    static_assert(PINYIN_CORRECT_ON_ONG == (1U << 28), "PINYIN_CORRECT_ON_ONG");
    static_assert(PINYIN_CORRECT_ALL == (0xffU << 21), "PINYIN_CORRECT_ALL");
    static_assert(DOUBLE_PINYIN_DEFAULT == DOUBLE_PINYIN_MS, "DOUBLE_PINYIN_DEFAULT");
    static_assert(ZHUYIN_DEFAULT == ZHUYIN_STANDARD, "ZHUYIN_DEFAULT");
    static_assert(PHRASE_MASK == 0x00FFFFFFU, "PHRASE_MASK");
    static_assert(PHRASE_INDEX_LIBRARY_MASK == 0x0F000000U, "PHRASE_INDEX_LIBRARY_MASK");
    static_assert(ADDON_DICTIONARY == 5, "ADDON_DICTIONARY");
    static_assert(NETWORK_DICTIONARY == 6, "NETWORK_DICTIONARY");
    static_assert(USER_DICTIONARY == 7, "USER_DICTIONARY");
    static_assert(null_token == 0, "null_token");
    static_assert(
        PHRASE_INDEX_MAKE_TOKEN(USER_DICTIONARY, null_token) ==
            (static_cast<uint32_t>(USER_DICTIONARY << 24) &
             PHRASE_INDEX_LIBRARY_MASK),
        "PHRASE_INDEX_MAKE_TOKEN");

    Session session;
    session.context = pinyin_init(argv[1], argv[2]);
    if (session.context == nullptr) {
        return 1;
    }
    session.instance = pinyin_alloc_instance(session.context);
    if (session.instance == nullptr) {
        return 2;
    }

    // The W8 gap symbol: 0 after allocation, stored parse result after
    // pinyin_parse_more_full_pinyins, and 0 again after pinyin_reset.
    if (pinyin_get_parsed_input_length(session.instance) != 0) {
        return 3;
    }
    const char *nihao = "nihao";
    size_t consumed = pinyin_parse_more_full_pinyins(session.instance, nihao);
    if (consumed != std::strlen(nihao) ||
        pinyin_get_parsed_input_length(session.instance) != consumed) {
        return 4;
    }
    if (!pinyin_reset(session.instance) ||
        pinyin_get_parsed_input_length(session.instance) != 0) {
        return 5;
    }

    static_assert(sizeof(PinyinKey *) == sizeof(void *), "PinyinKey");
    static_assert(sizeof(PinyinKeyPos *) == sizeof(void *), "PinyinKeyPos");
    return 0;
}
