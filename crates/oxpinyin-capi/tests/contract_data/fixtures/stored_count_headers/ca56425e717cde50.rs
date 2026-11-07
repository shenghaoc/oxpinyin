{
    use crate::support::Raw::*;
    static RAW: crate::support::Raw = L(&[L(&[S("0"), L(&[B(false), I(0)])]), L(&[S("1"), L(&[B(true), I(3)])]), L(&[S("16777215"), L(&[B(false), I(0)])]), L(&[S("2"), L(&[B(false), I(0)])]), L(&[S("3"), L(&[B(false), I(0)])]), L(&[S("4"), L(&[B(false), I(0)])]), L(&[S("5"), L(&[B(true), I(3)])]), L(&[S("6"), L(&[B(false), I(0)])]), L(&[S("no sub-index"), L(&[B(false), I(0)])]), L(&[S("tag 0x10000000"), L(&[B(true), I(3)])]), L(&[S("tag 0xf0000000"), L(&[B(true), I(3)])])]);
    crate::support::fixture(&RAW)
}
