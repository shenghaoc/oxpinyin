{
    use crate::support::Raw::*;
    static RAW: crate::support::Raw = L(&[S("你好"), B(true), L(&[L(&[B(true), L(&[S("你好")])]), L(&[B(true), L(&[S("你好")])]), L(&[B(true), L(&[S("你好")])])]), B(true), I(2)]);
    crate::support::fixture(&RAW)
}
