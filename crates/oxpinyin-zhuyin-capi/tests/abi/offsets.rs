use std::ffi::{CString, c_char, c_int, c_void};
use std::path::Path;
use std::ptr;

use zhuyin_capi::{zhuyin_fini, zhuyin_init};

// These exports are declared in zhuyin.h. The test only passes opaque handles
// back through the C interface.
unsafe extern "C" {
    fn zhuyin_alloc_instance(context: *mut c_void) -> *mut c_void;
    fn zhuyin_free_instance(instance: *mut c_void);
    fn zhuyin_set_options(context: *mut c_void, options: u32) -> bool;
    fn zhuyin_set_full_pinyin_scheme(context: *mut c_void, scheme: c_int) -> bool;
    fn zhuyin_parse_more_full_pinyins(instance: *mut c_void, input: *const c_char) -> usize;
    fn zhuyin_get_zhuyin_key_rest(
        instance: *mut c_void,
        offset: usize,
        rest: *mut *mut c_void,
    ) -> bool;
    fn zhuyin_get_left_zhuyin_offset(
        instance: *mut c_void,
        offset: usize,
        left: *mut usize,
    ) -> bool;
}

#[test]
fn luoma_left_offset_reads_the_unsplit_matrix() {
    let system = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/w3")
        .join(oxpinyin_data::DEFAULT_STORE_EXT);
    let system = CString::new(system.as_os_str().as_encoded_bytes()).expect("fixture path");
    let context = zhuyin_init(system.as_ptr(), ptr::null());
    assert!(!context.is_null());

    // SAFETY: The context came from zhuyin_init, the instance comes from
    // zhuyin_alloc_instance, and the C calls receive live output slots.
    let (left, has_middle_key) = unsafe {
        assert!(zhuyin_set_options(context.cast(), 0x8 | (1 << 7)));
        assert!(zhuyin_set_full_pinyin_scheme(context.cast(), 2));
        let instance = zhuyin_alloc_instance(context.cast());
        assert!(!instance.is_null());
        assert_eq!(
            zhuyin_parse_more_full_pinyins(instance, c"sian".as_ptr()),
            4
        );

        let mut left = usize::MAX;
        assert!(zhuyin_get_left_zhuyin_offset(instance, 4, &raw mut left));
        let mut rest = ptr::null_mut();
        let has_middle_key = zhuyin_get_zhuyin_key_rest(instance, 2, &raw mut rest);
        zhuyin_free_instance(instance);
        (left, has_middle_key)
    };
    zhuyin_fini(context);

    // Pin 074a2219 zhuyin.cpp:1017-1042 fills and fuzzes the parsed xian
    // key over 0..4. Column 2 stays empty, so the backward scan reaches
    // 0 (:1994-2023) and the key-rest getter refuses at 2 (:1908-1918).
    assert_eq!(left, 0);
    assert!(!has_middle_key);
}
