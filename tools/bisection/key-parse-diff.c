/*
 * key-parse-diff.c — every single-key parse entry point over every scheme
 * and a fixed option set, raw ChewingKey bytes and return value (#547).
 *
 * The three one-key probes (`pinyin_parse_full_pinyin`,
 * `pinyin_parse_double_pinyin`, `pinyin_parse_chewing`) run on the
 * context's live parsers under the context's live option word
 * (`pinyin.cpp:1484-1495`, `:1533-1544`, `:1577-1590` at 074a2219), so a
 * differential must pin both: every probe below follows an explicit
 * `pinyin_set_options` and an explicit scheme setter. The audit's
 * unpinned run (#547) counted the default option word's difference
 * (#532) as parse differences; with the word pinned, what remained was
 * the full-pinyin probe ignoring an unset PINYIN_INCOMPLETE and ignoring
 * the LUOMA / SECONDARY_ZHUYIN scheme — both fixed with this driver.
 *
 * Usage: ./key-parse-diff <so> <systemdir>
 *
 * The syllable list below is the first field of every row of the pin's
 * `pinyin_index[]` (`pinyin_parser_table.h`), spellings and correction
 * aliases alike. An apostrophe is never probed: the pin asserts on one
 * inside `parse_one_key` (`pinyin_parser2.cpp:170`). ZHUYIN_STANDARD_DVORAK
 * (7) is skipped because `pinyin_set_zhuyin_scheme` aborts the pin on it.
 *
 * This file is part of oxpinyin, GPL-3.0-or-later like the rest of it.
 *
 * For each option word W (set explicitly on both sides so the default
 * option word, #532, cannot enter), the full-pinyin entry is probed under
 * each FullPinyinScheme (1..3) over every string of 1..6 lowercase letters
 * that is a prefix of some pinyin syllable, plus that syllable with tone
 * digits 0..5 and a few junk shapes; the double-pinyin entry under schemes
 * 1..6 over every one- and two-character string of [a-z;]; the chewing
 * entry under zhuyin schemes 1..9 except 7 (7 aborts the pin) over every
 * one- and two-character string of the printable keyboard set and every
 * three-character string whose first two are a parsed pair. The key is
 * prefilled with 0xAB so an untouched out-param is visible.
 */
#define _POSIX_C_SOURCE 200809L
#include <dlfcn.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
typedef void ctx_t; typedef void inst_t;
typedef struct { uint16_t bits; } Key;
static void *lib;
static void *sym(const char *n){void *p=dlsym(lib,n); if(!p){fprintf(stderr,"missing %s\n",n);exit(1);} return p;}
static const uint32_t WORDS[] = {0x0, 0x8, 0x20, 0x28, 0x18a, 0x1aa, 0x1ea, 0x1ca, 0x3fe00000, 0x1fe00018, 0x1fe0039a, 0xffffffff};
static const char *SYLLABLES[] = {
    "a", "agn", "ai", "amg", "an", "ang", "ao", "au", "b", "ba",
    "bagn", "bai", "bamg", "ban", "bang", "bao", "bau", "begn", "bei", "bemg",
    "ben", "beng", "bi", "bian", "biao", "biau", "bie", "bieh", "bign", "bimg",
    "bin", "bing", "bo", "bu", "c", "ca", "cagn", "cai", "camg", "can",
    "cang", "cao", "ce", "cegn", "cemg", "cen", "ceng", "ch", "cha", "chagn",
    "chai", "chamg", "chan", "chang", "chao", "chau", "che", "chegn", "chemg", "chen",
    "cheng", "chi", "chia", "chian", "chiang", "chiao", "chiau", "chie", "chieh", "chih",
    "chin", "ching", "chiou", "chiu", "chiuan", "chiue", "chiun", "chiung", "chogn", "chomg",
    "chon", "chong", "chou", "chr", "chu", "chua", "chuagn", "chuai", "chuamg", "chuan",
    "chuang", "chuei", "chuen", "chui", "chun", "chung", "chuo", "chyong", "chyu", "chyuan",
    "chyueh", "chyun", "ci", "cogn", "comg", "con", "cong", "cou", "cu", "cuan",
    "cuei", "cuen", "cui", "cun", "cuo", "d", "da", "dagn", "dai", "damg",
    "dan", "dang", "dao", "dau", "de", "degn", "dei", "demg", "den", "deng",
    "di", "dia", "dian", "diao", "diau", "die", "dieh", "dign", "dimg", "din",
    "ding", "diou", "diu", "dogn", "domg", "don", "dong", "dou", "du", "duan",
    "duei", "duen", "dui", "dun", "dung", "duo", "e", "egn", "ei", "emg",
    "en", "eng", "er", "f", "fa", "fagn", "famg", "fan", "fang", "fe",
    "fegn", "fei", "femg", "fen", "feng", "fo", "fou", "fu", "g", "ga",
    "gagn", "gai", "gamg", "gan", "gang", "gao", "gau", "ge", "gegn", "gei",
    "gemg", "gen", "geng", "gogn", "gomg", "gon", "gong", "gou", "gu", "gua",
    "guagn", "guai", "guamg", "guan", "guang", "guei", "guen", "gui", "gun", "gung",
    "guo", "h", "ha", "hagn", "hai", "hamg", "han", "hang", "hao", "hau",
    "he", "hegn", "hei", "hemg", "hen", "heng", "hogn", "homg", "hon", "hong",
    "hou", "hu", "hua", "huagn", "huai", "huamg", "huan", "huang", "huei", "huen",
    "hui", "hun", "hung", "huo", "j", "ja", "jai", "jan", "jang", "jau",
    "je", "jei", "jen", "jeng", "jha", "jhai", "jhan", "jhang", "jhao", "jhe",
    "jhei", "jhen", "jheng", "jhih", "jhong", "jhou", "jhu", "jhua", "jhuai", "jhuan",
    "jhuang", "jhuei", "jhun", "jhuo", "ji", "jia", "jiagn", "jiamg", "jian", "jiang",
    "jiao", "jiau", "jie", "jieh", "jign", "jimg", "jin", "jing", "jiogn", "jiomg",
    "jion", "jiong", "jiou", "jiu", "jiuan", "jiue", "jiun", "jiung", "jou", "jr",
    "ju", "jua", "juai", "juan", "juang", "jue", "juei", "juen", "jun", "jung",
    "juo", "jv", "jvan", "jve", "jvn", "jyong", "jyu", "jyuan", "jyueh", "jyun",
    "k", "ka", "kagn", "kai", "kamg", "kan", "kang", "kao", "kau", "ke",
    "kegn", "kei", "kemg", "ken", "keng", "kogn", "komg", "kon", "kong", "kou",
    "ku", "kua", "kuagn", "kuai", "kuamg", "kuan", "kuang", "kuei", "kuen", "kui",
    "kun", "kung", "kuo", "l", "la", "lagn", "lai", "lamg", "lan", "lang",
    "lao", "lau", "le", "legn", "lei", "lemg", "len", "leng", "li", "lia",
    "liagn", "liamg", "lian", "liang", "liao", "liau", "lie", "lieh", "lign", "limg",
    "lin", "ling", "liou", "liu", "liue", "lo", "logn", "lomg", "lon", "long",
    "lou", "lu", "luan", "lue", "luen", "lun", "lung", "luo", "lv", "lve",
    "lyu", "lyueh", "m", "ma", "magn", "mai", "mamg", "man", "mang", "mao",
    "mau", "me", "megn", "mei", "memg", "men", "meng", "mi", "mian", "miao",
    "miau", "mie", "mieh", "mign", "mimg", "min", "ming", "miou", "miu", "mo",
    "mou", "mu", "n", "na", "nagn", "nai", "namg", "nan", "nang", "nao",
    "nau", "ne", "negn", "nei", "nemg", "nen", "neng", "ng", "ni", "nia",
    "niagn", "niamg", "nian", "niang", "niao", "niau", "nie", "nieh", "nign", "nimg",
    "nin", "ning", "niou", "niu", "niue", "nogn", "nomg", "non", "nong", "nou",
    "nu", "nuan", "nue", "nuen", "nun", "nung", "nuo", "nv", "nve", "nyu",
    "nyueh", "o", "ou", "p", "pa", "pagn", "pai", "pamg", "pan", "pang",
    "pao", "pau", "pegn", "pei", "pemg", "pen", "peng", "pi", "pian", "piao",
    "piau", "pie", "pieh", "pign", "pimg", "pin", "ping", "po", "pou", "pu",
    "q", "qi", "qia", "qiagn", "qiamg", "qian", "qiang", "qiao", "qie", "qign",
    "qimg", "qin", "qing", "qiogn", "qiomg", "qion", "qiong", "qiou", "qiu", "qu",
    "quan", "que", "quen", "qun", "qv", "qvan", "qve", "qvn", "r", "ragn",
    "ramg", "ran", "rang", "rao", "rau", "re", "regn", "remg", "ren", "reng",
    "ri", "rih", "rogn", "romg", "ron", "rong", "rou", "ru", "rua", "ruan",
    "ruei", "ruen", "rui", "run", "rung", "ruo", "s", "sa", "sagn", "sai",
    "samg", "san", "sang", "sao", "sau", "se", "segn", "semg", "sen", "seng",
    "sh", "sha", "shagn", "shai", "shamg", "shan", "shang", "shao", "shau", "she",
    "shegn", "shei", "shemg", "shen", "sheng", "shi", "shia", "shian", "shiang", "shiau",
    "shie", "shih", "shin", "shing", "shiou", "shiu", "shiuan", "shiue", "shiun", "shiung",
    "shou", "shr", "shu", "shua", "shuagn", "shuai", "shuamg", "shuan", "shuang", "shuei",
    "shuen", "shui", "shun", "shuo", "si", "sia", "sian", "siang", "siao", "sieh",
    "sih", "sin", "sing", "siou", "sogn", "somg", "son", "song", "sou", "su",
    "sua", "suan", "suei", "suen", "sui", "sun", "sung", "suo", "syong", "syu",
    "syuan", "syueh", "syun", "sz", "t", "ta", "tagn", "tai", "tamg", "tan",
    "tang", "tao", "tau", "te", "tegn", "temg", "teng", "ti", "tian", "tiao",
    "tiau", "tie", "tieh", "tign", "timg", "ting", "togn", "tomg", "ton", "tong",
    "tou", "tsa", "tsai", "tsan", "tsang", "tsao", "tsau", "tse", "tsen", "tseng",
    "tsih", "tsong", "tsou", "tsu", "tsuan", "tsuei", "tsun", "tsung", "tsuo", "tsz",
    "tu", "tuan", "tuei", "tuen", "tui", "tun", "tung", "tuo", "tz", "tza",
    "tzai", "tzan", "tzang", "tzau", "tze", "tzei", "tzen", "tzeng", "tzou", "tzu",
    "tzuan", "tzuei", "tzuen", "tzung", "tzuo", "w", "wa", "wagn", "wai", "wamg",
    "wan", "wang", "wegn", "wei", "wemg", "wen", "weng", "wo", "wong", "wu",
    "wun", "x", "xi", "xia", "xiagn", "xiamg", "xian", "xiang", "xiao", "xie",
    "xign", "ximg", "xin", "xing", "xiogn", "xiomg", "xion", "xiong", "xiou", "xiu",
    "xu", "xuan", "xue", "xuen", "xun", "xv", "xvan", "xve", "xvn", "y",
    "ya", "yagn", "yai", "yamg", "yan", "yang", "yao", "yau", "ye", "yeh",
    "yi", "yign", "yimg", "yin", "ying", "yo", "yogn", "yomg", "yon", "yong",
    "you", "yu", "yuan", "yue", "yueh", "yuen", "yun", "yung", "yv", "yvan",
    "yve", "yvn", "z", "za", "zagn", "zai", "zamg", "zan", "zang", "zao",
    "ze", "zegn", "zei", "zemg", "zen", "zeng", "zh", "zha", "zhagn", "zhai",
    "zhamg", "zhan", "zhang", "zhao", "zhe", "zhegn", "zhei", "zhemg", "zhen", "zheng",
    "zhi", "zhogn", "zhomg", "zhon", "zhong", "zhou", "zhu", "zhua", "zhuagn", "zhuai",
    "zhuamg", "zhuan", "zhuang", "zhuei", "zhuen", "zhui", "zhun", "zhuo", "zi", "zih",
    "zogn", "zomg", "zon", "zong", "zou", "zu", "zua", "zuan", "zuei", "zuen",
    "zui", "zun", "zuo",
};
static void probe(const char *fam, int scheme, uint32_t w, bool (*fn)(inst_t*,const char*,Key*), inst_t *inst, const char *in) {
    Key k; k.bits = 0xabab; bool ok = fn(inst, in, &k);
    printf("%s|%d|%#x|%s|%d|%04x\n", fam, scheme, w, in, ok, k.bits);
}
int main(int argc, char **argv) {
    if (argc != 3) return 1;
    lib = dlopen(argv[1], RTLD_NOW); if (!lib) { fprintf(stderr, "%s\n", dlerror()); return 1; }
    ctx_t *(*init)(const char*,const char*) = sym("pinyin_init");
    inst_t *(*alloc)(ctx_t*) = sym("pinyin_alloc_instance");
    bool (*set_options)(ctx_t*,uint32_t) = sym("pinyin_set_options");
    bool (*set_full)(ctx_t*,int) = sym("pinyin_set_full_pinyin_scheme");
    bool (*set_double)(ctx_t*,int) = sym("pinyin_set_double_pinyin_scheme");
    bool (*set_zhuyin)(ctx_t*,int) = sym("pinyin_set_zhuyin_scheme");
    bool (*parse_full)(inst_t*,const char*,Key*) = sym("pinyin_parse_full_pinyin");
    bool (*parse_double)(inst_t*,const char*,Key*) = sym("pinyin_parse_double_pinyin");
    bool (*parse_chewing)(inst_t*,const char*,Key*) = sym("pinyin_parse_chewing");
    ctx_t *ctx = init(argv[2], ""); if (!ctx) return 1;
    inst_t *inst = alloc(ctx);
    size_t nsyl = sizeof(SYLLABLES)/sizeof(SYLLABLES[0]);
    /* full pinyin inputs: every prefix of every syllable, syllables with tones, junk */
    static char inputs[8192][12]; size_t ni = 0;
    for (size_t s = 0; s < nsyl; ++s) {
        size_t len = strlen(SYLLABLES[s]);
        for (size_t l = 1; l <= len; ++l) { char buf[12]; memcpy(buf, SYLLABLES[s], l); buf[l] = 0; bool dup = false; for (size_t i = 0; i < ni; ++i) if (!strcmp(inputs[i], buf)) { dup = true; break; } if (!dup && ni < 8192) strcpy(inputs[ni++], buf); }
        for (int t = 0; t <= 6; ++t) { char buf[12]; snprintf(buf, sizeof buf, "%s%d", SYLLABLES[s], t); if (ni < 8192) strcpy(inputs[ni++], buf); }
    }
    const char *junk[] = {"", "v", "ue", "NI", "ni hao", "nih", "qqq", "x", "ng", "hm", "m", "a1", "er", "ei2", "lv", "lv3", "nve", "jv", "u", "i", "ii", "uu"};
    for (size_t j = 0; j < sizeof(junk)/sizeof(junk[0]); ++j) if (ni < 8192) strcpy(inputs[ni++], junk[j]);
    for (size_t w = 0; w < sizeof(WORDS)/sizeof(WORDS[0]); ++w) {
        printf("options|%#x|%d\n", WORDS[w], set_options(ctx, WORDS[w]));
        for (int scheme = 1; scheme <= 3; ++scheme) {
            printf("full-scheme|%d|%d\n", scheme, set_full(ctx, scheme));
            for (size_t i = 0; i < ni; ++i) probe("full", scheme, WORDS[w], parse_full, inst, inputs[i]);
        }
        set_full(ctx, 1);
        static const char dchars[] = "abcdefghijklmnopqrstuvwxyz;";
        for (int scheme = 1; scheme <= 6; ++scheme) {
            printf("double-scheme|%d|%d\n", scheme, set_double(ctx, scheme));
            char buf[4];
            for (size_t a = 0; a < 27; ++a) { buf[0] = dchars[a]; buf[1] = 0; probe("double", scheme, WORDS[w], parse_double, inst, buf);
                for (size_t b = 0; b < 27; ++b) { buf[1] = dchars[b]; buf[2] = 0; probe("double", scheme, WORDS[w], parse_double, inst, buf); } }
            const char *dj[] = {"", "ni3", "zhuang", "1a", "xyz", "ni9", "aaa", "zz;"};
            for (size_t j = 0; j < sizeof(dj)/sizeof(dj[0]); ++j) probe("double", scheme, WORDS[w], parse_double, inst, dj[j]);
        }
        static const char cchars[] = "abcdefghijklmnopqrstuvwxyz0123456789,-./;'[]=` ";
        const int zschemes[] = {1,2,3,4,5,6,8,9};
        for (size_t zi = 0; zi < 8; ++zi) {
            int scheme = zschemes[zi];
            printf("zhuyin-scheme|%d|%d\n", scheme, set_zhuyin(ctx, scheme));
            size_t nc = strlen(cchars); char buf[4];
            for (size_t a = 0; a < nc; ++a) { buf[0] = cchars[a]; buf[1] = 0; probe("chewing", scheme, WORDS[w], parse_chewing, inst, buf);
                for (size_t b = 0; b < nc; ++b) { buf[1] = cchars[b]; buf[2] = 0; Key k; k.bits = 0xabab; bool ok = parse_chewing(inst, buf, &k); printf("chewing|%d|%#x|%s|%d|%04x\n", scheme, WORDS[w], buf, ok, k.bits);
                    if (ok) {
                        for (size_t c = 0; c < nc; ++c) { buf[2] = cchars[c]; buf[3] = 0; probe("chewing", scheme, WORDS[w], parse_chewing, inst, buf); }
                    }
                    buf[2] = 0; } }
        }
    }
    return 0;
}
