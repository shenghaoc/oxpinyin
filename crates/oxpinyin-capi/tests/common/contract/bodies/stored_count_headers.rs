//! Stored pronunciation-count headers (#882, #1549).

use std::collections::BTreeMap;

use crate::facade;
use crate::fixtures::{ConfEdit, chunk, conf_system, write_user_conf};
use crate::support::{Kit, UNTOUCHED};

/// A sub-index image: the header words, the `#`-separated offset table and
/// content (`phrase_index.cpp:180-205`).
fn sub_index(total: u32, offsets: &[u32], content: &[u8]) -> Vec<u8> {
    let count = u32::try_from(offsets.len()).expect("offsets");
    let index_two = 17 + count * 4 + 1;
    let content_len = u32::try_from(content.len()).expect("content");
    let mut payload: Vec<u8> = [total, 17, index_two, index_two + content_len + 1]
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    payload.push(b'#');
    payload.extend(offsets.iter().flat_map(|value| value.to_le_bytes()));
    payload.push(b'#');
    payload.extend_from_slice(content);
    payload.push(b'#');
    payload
}

fn count_of(inst: crate::support::P, token: u32) -> (bool, u32) {
    let mut number = UNTOUCHED;
    let ret = facade::token_get_n_pronunciation(inst, token, &raw mut number);
    (ret, number)
}

/// Planted count headers in the system library 1 or the user library 7, read
/// at every slot, under two tag bits and in a library with no sub-index.
pub fn stored_count_headers(k: &mut Kit, user: bool) -> BTreeMap<String, (bool, u32)> {
    let item: Vec<u8> = [
        &[1_u8, 3][..],
        &9_u32.to_le_bytes(),
        &55296_u32.to_le_bytes(),
        &[0xff; 18],
    ]
    .concat();
    let content: Vec<u8> = [&[0_u8; 8][..], &item, &[1]].concat();
    let len = u32::try_from(content.len()).expect("content");
    let payload = sub_index(9, &[0, 8, len - 1, 0, len, 8], &content);
    let (ctx, library) = if user {
        write_user_conf(k);
        std::fs::write(k.user.join("user.bin"), chunk(&payload)).expect("user.bin");
        (k.init_default(), 7_u32)
    } else {
        let system = conf_system(k, &[ConfEdit::Replace("gb_char.bin", "count.bin")]);
        std::fs::write(system.join("count.bin"), chunk(&payload)).expect("count.bin");
        (k.init(Some(&system), Some(&k.user.clone())), 1)
    };
    assert!(!ctx.is_null(), "count fixture init");
    let inst = facade::alloc_instance(ctx);
    let mut out = BTreeMap::new();
    for slot in [0_u32, 1, 2, 3, 4, 5, 6, 16_777_215] {
        out.insert(slot.to_string(), count_of(inst, (library << 24) | slot));
    }
    for tag in [0x1000_0000_u32, 0xf000_0000] {
        out.insert(
            format!("tag {tag:#x}"),
            count_of(inst, tag | (library << 24) | 1),
        );
    }
    out.insert("no sub-index".to_owned(), count_of(inst, 0x0f00_0001));
    facade::free_instance(inst);
    facade::fini(ctx);
    out
}

#[derive(Debug, PartialEq)]
pub struct Truncated {
    pub count: (bool, u32),
}

/// A header whose declared body runs past the content: the count is still
/// answered (`phrase_index.cpp:203-205`).
pub fn untouched_truncated(k: &mut Kit) -> Truncated {
    let content: Vec<u8> = [&[0_u8; 8][..], &[255, 17]].concat();
    let payload = sub_index(0, &[0, 8, 9, 10, 0], &content);
    let system = conf_system(k, &[ConfEdit::Replace("gb_char.bin", "count.bin")]);
    std::fs::write(system.join("count.bin"), chunk(&payload)).expect("count.bin");
    let ctx = k.init(Some(&system), None);
    assert!(!ctx.is_null(), "count fixture init");
    let inst = facade::alloc_instance(ctx);
    let count = count_of(inst, 0x0100_0001);
    facade::free_instance(inst);
    facade::fini(ctx);
    Truncated { count }
}
