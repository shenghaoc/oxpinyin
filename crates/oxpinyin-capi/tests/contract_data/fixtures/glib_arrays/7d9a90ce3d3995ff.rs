{
    use crate::support::Raw::*;
    static RAW: crate::support::Raw = L(&[L(&[B(true), L(&[I(0x1006205)]), B(false), L(&[I(0x1006205)]), B(true)]), L(&[B(true), L(&[I(43), I(646)]), B(false), L(&[I(43), I(646)]), B(true)])]);
    crate::support::fixture(&RAW)
}
