//! `pinyin_alloc_instance` on a finalised context (#528).

use crate::facade;
use crate::support::Kit;

#[derive(Debug, PartialEq)]
pub struct AfterFini {
    pub live_instance: bool,
}

/// Allocates and frees an instance, finalises the context, then allocates again.
/// The pin reads the freed context (`pinyin.cpp:1321-1322`) and survives by
/// chance; its second answer is indeterminate, so only the exit status and the
/// first answer are held.
pub fn alloc_after_fini(k: &mut Kit) -> AfterFini {
    let context = k.init_default();
    let live = facade::alloc_instance(context);
    facade::free_instance(live);
    facade::fini(context);
    let _after = facade::alloc_instance(context);
    AfterFini {
        live_instance: !live.is_null(),
    }
}
