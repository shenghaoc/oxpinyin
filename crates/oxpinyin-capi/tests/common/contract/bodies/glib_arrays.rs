//! Caller-owned GArrays (#924): real arrays the facade grows and shrinks, NULL
//! arrays, and the phrase-segment critical (#1297).

use std::sync::Mutex;

use crate::facade;
use crate::readers::{c, tokens_of};
use crate::support::{Kit, Log, P, UNTOUCHED};

/// What the arrays' clear callback saw, in order.
static CLEARED: Mutex<Vec<u32>> = Mutex::new(Vec::new());
/// The element width of the array under test, for the clear callback.
static WIDTH: Mutex<usize> = Mutex::new(4);

extern "C" fn clear(element: glib_sys::gpointer) {
    let width = *WIDTH
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // SAFETY: GLib hands the callback a pointer to one live element of the
    // array, `width` bytes wide.
    let value = unsafe {
        if width == 4 {
            element.cast::<u32>().read_unaligned()
        } else {
            u32::from(element.cast::<u16>().read_unaligned())
        }
    };
    CLEARED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(value);
}

fn take_cleared() -> Vec<u32> {
    std::mem::take(
        &mut *CLEARED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    )
}

/// The live payload, checking the zero terminator the array promises.
fn values(array: *mut glib_sys::GArray, width: usize) -> Vec<u32> {
    // SAFETY: a live zero-terminated GArray of `width`-byte elements: `data`
    // holds `len` of them and one more.
    unsafe {
        let view = &*array;
        let len = view.len as usize;
        let read = |index: usize| -> u32 {
            if width == 4 {
                view.data.cast::<u32>().add(index).read_unaligned()
            } else {
                u32::from(view.data.cast::<u16>().add(index).read_unaligned())
            }
        };
        assert_eq!(read(len), 0, "the zero terminator");
        (0..len).map(read).collect()
    }
}

#[derive(Debug, PartialEq)]
pub struct Arrays {
    pub ret: bool,
    pub original: Vec<u32>,
    pub refused: bool,
    pub cleared: Vec<u32>,
    pub caller_mutation: bool,
}

#[derive(Debug, PartialEq)]
pub struct Lifecycle {
    pub tokens: Arrays,
    pub keys: Arrays,
}

/// Mutates, resizes, reuses and frees both real caller-owned GArray types
/// (`pinyin.cpp:2652-2667`, `:2805-2817`; `phrase_large_table3.h:77-97`).
pub fn array_lifecycle(k: &mut Kit) -> Lifecycle {
    let inst = k.inst();
    let mut token = 0_u32;
    let mut run = |width: usize| -> Arrays {
        *WIDTH
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = width;
        take_cleared();
        // SAFETY: a fresh zero-terminated, cleared GArray of `width`-byte elements.
        let array = unsafe { glib_sys::g_array_new(1, 1, u32::try_from(width).expect("width")) };
        // SAFETY: the callback reads one element of the array's width.
        unsafe { glib_sys::g_array_set_clear_func(array, Some(clear)) };
        let seed: u32 = 0x1234;
        // SAFETY: one element of `width` bytes from `seed` (little-endian low bytes).
        unsafe { glib_sys::g_array_append_vals(array, (&raw const seed).cast(), 1) };
        let ret = if width == 4 {
            facade::lookup_tokens(inst, c("你好").as_ptr(), array.cast())
        } else {
            facade::token_get_nth_pronunciation(inst, token, 0, array.cast())
        };
        assert!(ret && take_cleared() == [0x1234]);
        let original = values(array, width);
        assert!(!original.is_empty());
        if width == 4 {
            token = original[0];
        }
        // Grow past the spare capacity, then shrink and reuse it.
        let extra: Vec<u8> = (1..=257_u32)
            .flat_map(|value| {
                if width == 4 {
                    value.to_le_bytes().to_vec()
                } else {
                    u16::try_from(value).expect("u16").to_le_bytes().to_vec()
                }
            })
            .collect();
        // SAFETY: 257 elements of `width` bytes.
        unsafe { glib_sys::g_array_append_vals(array, extra.as_ptr().cast(), 257) };
        assert_eq!(
            values(array, width),
            [original.clone(), (1..=257).collect()].concat()
        );
        // SAFETY: the array is live.
        unsafe { glib_sys::g_array_set_size(array, u32::try_from(original.len()).expect("len")) };
        assert_eq!(values(array, width), original);
        take_cleared();
        let refused = if width == 4 {
            facade::lookup_tokens(inst, c("not-a-phrase").as_ptr(), array.cast())
        } else {
            facade::token_get_nth_pronunciation(inst, 0xFFFF_FFFF, 0, array.cast())
        };
        let cleared = take_cleared();
        assert!(!refused && values(array, width).is_empty() && cleared == original);
        // SAFETY: the array is live.
        unsafe { glib_sys::g_array_set_size(array, 17) };
        assert_eq!(values(array, width), vec![0; 17]);
        // free(FALSE) hands the data to the caller.
        // SAFETY: the array is freed once; the data it hands back is freed once.
        unsafe { glib_sys::g_free(glib_sys::g_array_free(array, 0).cast()) };
        Arrays {
            ret,
            original,
            refused,
            cleared,
            caller_mutation: true,
        }
    };
    let tokens = run(4);
    let keys = run(2);
    Lifecycle { tokens, keys }
}

#[derive(Debug, PartialEq)]
pub struct Segment {
    pub ret: bool,
    pub retained: Vec<u32>,
    pub messages: Vec<Option<String>>,
}

/// The tokens `phrase_segment` left, read with initialised sentinels.
fn segment_result(inst: P) -> Vec<u32> {
    let mut count = UNTOUCHED;
    assert!(facade::get_n_phrase(inst, &raw mut count));
    (0..count)
        .map(|index| {
            let mut token = UNTOUCHED;
            assert!(facade::get_phrase_token(inst, index, &raw mut token));
            token
        })
        .collect()
}

/// Segments 你好, then invalid UTF-8: the earlier result stays
/// (`pinyin.cpp:1450-1454`, `zhuyin.cpp:969-973`).
pub fn segment_critical(k: &mut Kit) -> Segment {
    let inst = k.inst();
    assert!(facade::phrase_segment(inst, c("你好").as_ptr()));
    let old = segment_result(inst);
    assert!(old.iter().any(|&token| token != 0));
    k.clear_logs();
    let ret = facade::phrase_segment(inst, c(b"\xff\xfe").as_ptr());
    assert!(!ret && segment_result(inst) == old);
    Segment {
        ret,
        retained: old,
        messages: k.messages(),
    }
}

#[derive(Debug, PartialEq)]
pub struct NullProbe {
    pub ret: bool,
    pub logs: Vec<Log>,
    pub messages: Vec<Option<String>>,
}

#[derive(Debug, PartialEq)]
pub struct NullArray {
    pub found: NullProbe,
    pub missing: NullProbe,
}

/// `lookup_tokens` or `token_get_nth_pronunciation` with a NULL array, for a
/// found and a missing input: GLib's own criticals (`garray.c:611`, `:778`).
pub fn null_array(k: &mut Kit, pronunciation: bool) -> NullArray {
    let token = tokens_of(k, "你好")[0];
    let inst = k.inst();
    let probe = |found: bool| {
        k.clear_logs();
        let ret = if pronunciation {
            facade::token_get_nth_pronunciation(
                inst,
                if found { token } else { 0xFFFF_FFFF },
                0,
                std::ptr::null_mut(),
            )
        } else {
            facade::lookup_tokens(
                inst,
                c(if found { "你好" } else { "not-a-phrase" }).as_ptr(),
                std::ptr::null_mut(),
            )
        };
        NullProbe {
            ret,
            logs: k.logs(),
            messages: k.messages(),
        }
    };
    let found = probe(true);
    let missing = probe(false);
    NullArray { found, missing }
}
