{
    use crate::support::Raw::*;
    static RAW: crate::support::Raw = L(&[L(&[B(false), L(&[])]), L(&[B(true)]), L(&[B(true), L(&[I(0x7000001)])]), L(&[B(true)]), L(&[B(true), L(&[I(0x7000001)])]), B(true), L(&[B(true), L(&[I(0x7000001)])]), B(true), L(&[B(false), L(&[])]), L(&[B(true)]), L(&[B(true), L(&[I(0x7000001)])]), B(true), L(&[B(true), L(&[I(0x7000001)])]), L(&[L(&[B(true), L(&[S("龘龘")]), L(&[S("da2'da2")]), I(100)])])]);
    crate::support::fixture(&RAW)
}
