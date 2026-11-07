{
    use crate::support::Raw::*;
    static RAW: crate::support::Raw = L(&[L(&[S("parse_more_chewings b'\\xffsu3'"), I(0)]), L(&[S("parse_more_chewings b'su3\\xff'"), I(3)]), L(&[S("parse_more_chewings b'su\\xff3'"), I(0)]), L(&[S("parse_more_full_pinyins b'\\xffni3'"), I(0)]), L(&[S("parse_more_full_pinyins b'ni3\\xffhao3'"), I(3)]), L(&[S("parse_more_full_pinyins b'ni3hao3\\xff'"), I(7)])]);
    crate::support::fixture(&RAW)
}
