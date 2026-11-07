{
    use crate::support::Raw::*;
    static RAW: crate::support::Raw = L(&[I(0x2edfb12), L(&[I(8), B(true), L(&[L(&[S("你好")]), L(&[S("你好")]), L(&[S("你号")])]), L(&[L(&[L(&[I(1), S("你好")])]), L(&[L(&[I(1), S("你号")])]), L(&[L(&[I(2), S("你")])]), L(&[L(&[I(2), S("尼")])]), L(&[L(&[I(2), S("呢")])]), L(&[L(&[I(2), S("泥")])]), L(&[L(&[I(2), S("妮")])]), L(&[L(&[I(2), S("拟")])])]), L(&[L(&[S("你好"), L(&[B(true), L(&[I(0x1001225), I(0x10008a5)])])])])])]);
    crate::support::fixture(&RAW)
}
