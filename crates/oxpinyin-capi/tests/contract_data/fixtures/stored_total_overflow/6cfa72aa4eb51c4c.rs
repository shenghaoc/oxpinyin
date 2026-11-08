{
    use crate::support::Raw::*;
    static RAW: crate::support::Raw = L(&[L(&[B(true), I(31728)]), B(true), L(&[B(true), I(0x4cc72f)]), L(&[I(8), B(true), L(&[L(&[S("你号")]), L(&[S("尼号")]), L(&[S("你好")])]), L(&[L(&[L(&[I(1), S("你号")])]), L(&[L(&[I(1), S("尼号")])]), L(&[L(&[I(1), S("你好")])]), L(&[L(&[I(2), S("你")])]), L(&[L(&[I(2), S("尼")])]), L(&[L(&[I(2), S("呢")])]), L(&[L(&[I(2), S("泥")])]), L(&[L(&[I(2), S("妮")])])]), L(&[L(&[S("你好"), L(&[B(true), L(&[I(0x1001225), I(0x10008a5)])])])])]), Nil, Nil, Nil, Nil, Nil]);
    crate::support::fixture(&RAW)
}
