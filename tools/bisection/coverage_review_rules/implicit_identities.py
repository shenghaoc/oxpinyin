"""Exact synthetic/header identities reviewed against pin 074a2219.

GCC emits implicit constructor/destructor bodies and counter descriptors, but
public gcov output omits their artificial-function records in this build.
Absence of a JSON record says nothing about whether the function ran. These mappings cite
the actual owning fields and the Rust assembly/drop sites instead.

GCC 16's gcov.cc:1764-1766 removes artificial functions before output:
https://github.com/gcc-mirror/gcc/blob/releases/gcc-16.2.0/gcc/gcov.cc#L1764
"""

from coverage_reviews import identity_review

for mode, file, context_line, instance_line, factory in (
    ("pinyin", "src/pinyin.cpp", 39, 73, "pub(crate) fn try_new("),
    ("zhuyin", "src/zhuyin.cpp", 34, 60, "pub(crate) fn try_open("),
):
    crate = "crates/oxpinyin-" + ("zhuyin-" if mode == "zhuyin" else "") + "capi/src/"
    owner = "_" + mode
    for operation in ("", "~"):
        identity_review(
            f"signature:{file}:{context_line}:{owner}_context_t::{operation}{owner}_context_t()",
            [(crate + "state.rs", factory),
             (crate + "context.rs", f'pub extern "C" fn {mode}_fini('),
             ("crates/oxpinyin-facade/src/context.rs", "pub struct ContextCore {")],
            "The pin's implicit context special member initializes/destroys its value-owned SystemTableInfo2/UserTableInfo members; raw pointer fields are managed separately by init/fini. Rust folds value initialization into the facade's context factory and field construction, and field destruction into Box/ContextCore drop at fini. This maps only the synthetic lifetime body, not all init/fini failure behavior. GCC's object contains its exact ABI constructor/destructor symbol, but public gcov output omits its artificial-function record. That identity is resolved by the full ABI and defining type/source, with no fabricated execution count.",
            status="ABSENT_INLINE",
        )
        identity_review(
            f"signature:{file}:{instance_line}:{owner}_instance_t::{operation}{owner}_instance_t()",
            [(crate + "instance.rs", f'pub extern "C" fn {mode}_alloc_instance('),
             (crate + "instance.rs", f'pub extern "C" fn {mode}_free_instance('),
             ("crates/oxpinyin-facade/src/instance.rs", "pub(crate) const fn new(")],
            "The pin's implicit instance lifetime body constructs/destroys the value-owned PhoneticKeyMatrix and NBestMatchResults, with raw arrays initialized/freed by the surrounding alloc/free functions. Rust's InstanceCore field assembly and the C facade's Box drop own the corresponding session, parse caches and candidate vectors. No separate synthetic method is needed. GCC emits the full ABI body but public gcov output omits its artificial-function record; this exact source/object review accounts for it without declaring it unexecuted.",
            status="ABSENT_INLINE",
        )

for operation, line, suffix in (("==", 81, ""), ("==", 81, " (.4027)"),
                               ("==", 81, " (.4037)"), ("!=", 93, "")):
    identity_review(
        f"signature:src/storage/chewing_key.h:{line}:operator{operation}(_ChewingKey, _ChewingKey){suffix}",
        [("crates/oxpinyin-chewing/src/chewing_key.rs", "pub struct ChewingKey {")],
        "The immutable header compares exactly initial, middle, final and tone; inequality negates that comparison. Rust's derived PartialEq/Eq on ChewingKey compares the same four component fields. LLVM's local clones retain separate compile units. GCC's actual object and gcov symbols are _Zeq/_Zne; the hand-reviewed cross-compiler join requires the complete clang signature and parameter mangling, the exact pin/GCC definition extent and compile unit. Libclang omitted these G_BEGIN_DECLS body extents, so the two literal pin ranges were read directly; a conflicting available extent is rejected. The join does not use an operator name alone.",
    )

for signature, targets, reason in (
    ("src/storage/ngram.h:59:pinyin::SingleGram::~SingleGram()",
     [("crates/oxpinyin-data/src/bigram_table.rs", "pub fn load_successors("),
      ("crates/oxpinyin-data/src/single_gram.rs", "pub fn decode_single_gram(")],
     "SingleGram's implicit destructor destroys its MemoryChunk. Rust decodes a borrowed byte value into an owned successor Vec and drops those buffers with their local row; no manually owning SingleGram wrapper remains. This is a storage lifetime correspondence, not a claim about gram algorithms."),
    ("src/storage/phonetic_key_matrix.h:123:pinyin::PhoneticKeyMatrix::PhoneticKeyMatrix()",
     [("crates/oxpinyin-core/src/graph.rs", "pub struct SegmentGraph {")],
     "The implicit constructor constructs the two PhoneticTable members, whose own constructors create empty column arrays. Rust's default SegmentGraph owns initially empty edge/start/reachability Vecs; parsing builds this graph instead of two GLib column matrices. This accounts only for construction/storage, not the parser or decoder algorithm."),
    ("src/storage/phonetic_key_matrix.h:123:pinyin::PhoneticKeyMatrix::~PhoneticKeyMatrix()",
     [("crates/oxpinyin-core/src/graph.rs", "pub struct SegmentGraph {")],
     "The implicit destructor destroys both PhoneticTable members, releasing each GLib column array. SegmentGraph's owned edge/start/reachability Vecs release their storage through Rust field drop; there is no independent matrix destruction method."),
    ("src/storage/phrase_index.h:64:pinyin::PhraseItem::~PhraseItem()",
     [("crates/oxpinyin-data/src/phrase_library.rs", "pub struct PhraseItemView<'a> {")],
     "The implicit destructor destroys MemoryChunk. Rust's PhraseItemView borrows the library payload and owns no chunk, so dropping the view has no storage to release. The owning PhraseLibrary/mapping lifetime releases payload storage separately; no item destructor remains."),
    ("src/storage/phrase_large_table3.h:50:pinyin::PhraseTableEntry::~PhraseTableEntry()",
     [("crates/oxpinyin-data/src/phrase_table.rs", "pub(crate) fn search("),
      ("crates/oxpinyin-data/src/row_format.rs", "pub fn decode_tokens(")],
     "The implicit destructor destroys the entry's MemoryChunk. Rust holds a DBM byte value only during lookup and decodes its tokens into an owned Vec; ordinary byte-buffer/Vec drop replaces the reusable owning entry. Lookup/mutation algorithms are separate classifications."),
    ("src/storage/pinyin_parser2.h:95:pinyin::PhoneticParser2::PhoneticParser2()",
     [("crates/oxpinyin-core/src/parser.rs", "pub struct FullPinyinParser;"),
      ("crates/oxpinyin-core/src/scheme.rs", "pub struct DoublePinyinParser {")],
     "PhoneticParser2 has no data members; its implicit constructor establishes only C++ polymorphic base state. Rust uses concrete FullPinyinParser/DoublePinyinParser values and scheme dispatch, so it needs no base/vptr construction. Concrete parser state and parsing behavior are reviewed separately."),
    ("src/storage/zhuyin_parser2.h:49:pinyin::ZhuyinParser2::ZhuyinParser2()",
     [("crates/oxpinyin-core/src/scheme.rs", "pub struct ZhuyinParser {")],
     "ZhuyinParser2 adds no data members to PhoneticParser2; its implicit constructor establishes only polymorphic base/vptr state. Rust's concrete ZhuyinParser stores a scheme enum and dispatches without an owning C++ base object, so the synthetic base constructor is absent. Keyboard parsing is accounted for separately."),
):
    identity_review("signature:" + signature, targets,
                    reason + " GCC's complete object ABI symbol exists, while public gcov output omits the artificial-function record. Exact object/type correspondence and these pin/Rust sources resolve the identity; this source review supplies no inferred execution credit.",
                    status="ABSENT_INLINE")

for line, signature, usr in (
    (623, "parse_one_key", "c:@N@pinyin@S@PinyinDirectParser2@F@parse_one_key#i#&$@S@_ChewingKey#&S#*1C#I#1"),
    (675, "parse", "c:@N@pinyin@S@PinyinDirectParser2@F@parse#i#&*$@S@_GArray#S0_#*1C#I#1"),
):
    identity_review(
        usr, [],
        "Not constructible through the bdb C exports: PinyinDirectParser2 is a separate PhoneticParser2 subclass (pinyin_parser2.h:208-221), not a ZhuyinParser2 subclass. pinyin_init constructs only FullPinyinParser2, DoublePinyinParser2 and ZhuyinSimpleParser2 (pinyin.cpp:346-348); zhuyin_init constructs FullPinyinParser2 and ZhuyinSimpleParser2 (zhuyin.cpp:291-292). The chewing scheme setters replace the opaque context's parser only with Simple/Discrete/CP26 classes (pinyin.cpp:1159-1188, zhuyin.cpp:707-734). No exported setter accepts an externally constructed parser. The only additional stack constructions occur in bulk-text loaders: ChewingLargeTable::load_text (chewing_large_table.cpp:666-693), ChewingLargeTable2::load_text (chewing_large_table2.cpp:112-140), and FacadePhraseIndex::load_text (phrase_index.cpp:522-574). None is called or passed as a callback by a C-export path: init opens binary tables, while phrase import parses through the context-owned FullPinyinParser2. Every direct caller and function-address use in the configured 23 TUs was checked; those loader bodies have no export witness. The graph retains this conservative override candidate to preserve its original identity, with this explicit absence proof; missing counters are not the proof.",
        status="ABSENT_BDB_EXPORTS",
    )
