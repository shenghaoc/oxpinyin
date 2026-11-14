//! The zhuyin twins of `oxpinyin-capi`'s parse-matrix tests: the parsed modes
//! keep the matrix the pin built at the parse, and the offset and key
//! functions read it. The pin's zhuyin functions read every item of a column
//! in the character offset (`zhuyin.cpp:2117`) and the left offset (`:2009`),
//! and the first item of the column in the other getters
//! (`:1902`, `:1924`, `:1961`, `:2043-2053`). Register rows 19 and 66. The
//! pinyin twins hold the first-item reads, which go through the shared
//! facade code.

use crate::config::{zhuyin_set_chewing_scheme, zhuyin_set_options};
use crate::context::zhuyin_fini;
use crate::instance::zhuyin_free_instance;
use crate::iterators::{
    zhuyin_begin_add_phrases, zhuyin_end_add_phrases, zhuyin_iterator_add_phrase,
};
use crate::parse::zhuyin_parse_more_chewings;
use crate::sentence::zhuyin_get_character_offset;
use crate::test_support::{TempUserDir, cstr, open_with_user};
use crate::types::{ZhuyinContext, ZhuyinInstance};

const L_N: u32 = 1 << 15;

/// A context and instance that release themselves.
struct Fixture {
    context: *mut ZhuyinContext,
    instance: *mut ZhuyinInstance,
    _user_dir: TempUserDir,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let user_dir = TempUserDir::new(tag);
        let (context, instance) = open_with_user(&user_dir.path);
        Self {
            context,
            instance,
            _user_dir: user_dir,
        }
    }

    /// 弄 imported with the one reading ㄌㄨㄥ (`long`).
    fn with_long_only(tag: &str) -> Self {
        let fixture = Self::new(tag);
        let import = zhuyin_begin_add_phrases(fixture.context, 5);
        assert!(zhuyin_iterator_add_phrase(
            import,
            cstr("弄").as_ptr(),
            cstr("ㄌㄨㄥ").as_ptr(),
            1
        ));
        zhuyin_end_add_phrases(import);
        fixture
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        zhuyin_free_instance(self.instance);
        zhuyin_fini(self.context);
    }
}

#[test]
fn the_character_offset_reads_the_chewing_fuzzy_alternative() {
    let fixture = Fixture::with_long_only("z-char-offset-fuzzy");
    assert!(zhuyin_set_options(fixture.context, 0x8 | L_N));
    assert!(zhuyin_set_chewing_scheme(fixture.context, 1));
    assert_eq!(
        zhuyin_parse_more_chewings(fixture.instance, cstr("sj/").as_ptr()),
        3
    );
    let mut length = usize::MAX;
    assert!(zhuyin_get_character_offset(
        fixture.instance,
        cstr("弄").as_ptr(),
        2,
        &raw mut length
    ));
    // Offset 2, not the parsed length 3: at 3 the pin's walk reaches the zero
    // key and fails (`zhuyin.cpp:2108-2128`; see `zhuyin_get_character_offset`).
    // The key's end lies past offset 2, so no character is consumed yet; the
    // walk answers true only because `long` pronounces 弄.
    assert_eq!(length, 0);
}
