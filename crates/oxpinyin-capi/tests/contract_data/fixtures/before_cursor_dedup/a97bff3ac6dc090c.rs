{
    use crate::support::Raw::*;
    static RAW: crate::support::Raw = L(&[I(4), L(&[B(true), L(&[])]), L(&[B(true), L(&[L(&[L(&[I(2), S("甲乙")])])])]), B(true), L(&[B(true), L(&[L(&[L(&[I(1), S("甲乙")])])])]), L(&[B(true), L(&[L(&[L(&[I(2), S("甲乙")])])])])]);
    crate::support::fixture(&RAW)
}
