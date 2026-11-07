{
    use crate::support::Raw::*;
    static RAW: crate::support::Raw = L(&[B(true), L(&[B(true)]), L(&[B(true)]), L(&[B(true)]), L(&[L(&[S("addon.bin"), S("gb_char.dbin"), S("gbk_char.dbin"), S("merged.dbin"), S("network.bin"), S("opengram.dbin"), S("user.bin"), S("user.conf"), S("user_bigram.db"), S("user_phrase_index.bin"), S("user_pinyin_index.bin")])])]);
    crate::support::fixture(&RAW)
}
