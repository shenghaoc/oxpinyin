{
    use crate::support::Raw::*;
    static RAW: crate::support::Raw = L(&[S("0x1003e57"), I(10), L(&[B(true), I(67)]), L(&[S("癫痫"), I(2), I(10), L(&[B(true), I(67)]), B(true), B(true)]), L(&[B(true), I(550)])]);
    crate::support::fixture(&RAW)
}
