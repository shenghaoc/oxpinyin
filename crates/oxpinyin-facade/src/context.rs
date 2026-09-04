//! Live context state: the option/scheme word shared by a context and
//! every instance it allocated, plus the context-level open/save laws.

use std::path::Path;
use std::sync::Arc;

use oxpinyin_core::{DoublePinyinScheme, FullPinyinScheme, OptionBits, ZhuyinScheme};
use oxpinyin_engine::{Config, ConfigValue};
use oxpinyin_runtime::{Runtime, user_store_file};
use oxpinyin_user::UserStore;

/// The live option/scheme state a context owns and every instance it
/// allocates shares: `set_options`/`set_*_scheme` on the context remask
/// already-allocated instances through these handles.
///
/// Both C-ABI facades carry the same seven fields today (the zhuyin
/// facade adds `force_tone`, which its `zhuyin_set_options` writes); the
/// seed word differs per facade and is the caller's choice at
/// [`ContextCore::open`].
#[derive(Clone)]
pub struct LiveOptions {
    /// Live `PINYIN_INCOMPLETE` bit.
    pub incomplete: Arc<std::sync::atomic::AtomicBool>,
    /// Live double-pinyin scheme (header discriminant value).
    pub double_scheme: Arc<std::sync::atomic::AtomicI32>,
    /// Live Zhuyin scheme (header discriminant value).
    pub zhuyin_scheme: Arc<std::sync::atomic::AtomicI32>,
    /// Live full-pinyin scheme (header discriminant value).
    pub full_scheme: Arc<std::sync::atomic::AtomicI32>,
    /// Live `USE_TONE` bit.
    pub use_tone: Arc<std::sync::atomic::AtomicBool>,
    /// Live `FORCE_TONE` bit (nested under `USE_TONE` by the zhuyin
    /// parser; written by both facades' `set_options`, read by neither —
    /// the parsers take it off the option word itself).
    pub force_tone: Arc<std::sync::atomic::AtomicBool>,
    /// Live option word.
    pub options: Arc<std::sync::atomic::AtomicU32>,
}

impl LiveOptions {
    /// Seeds the live state the way an init does: the bools derive off
    /// `option_word` (so `pinyin_init`'s `PINYIN_INCOMPLETE` seeds
    /// incomplete ON, `zhuyin_init`'s `USE_TONE | FORCE_TONE` seeds both
    /// tone bits ON), and the three schemes start at the header defaults
    /// both facades share (MS double, Standard zhuyin, Hanyu full).
    #[must_use]
    pub fn new(option_word: u32) -> Self {
        let bits = OptionBits::from_bits(option_word);
        Self {
            incomplete: Arc::new(std::sync::atomic::AtomicBool::new(
                bits.contains(oxpinyin_core::PINYIN_INCOMPLETE),
            )),
            double_scheme: Arc::new(std::sync::atomic::AtomicI32::new(
                DoublePinyinScheme::Ms as i32,
            )),
            zhuyin_scheme: Arc::new(std::sync::atomic::AtomicI32::new(
                ZhuyinScheme::Standard as i32,
            )),
            full_scheme: Arc::new(std::sync::atomic::AtomicI32::new(
                FullPinyinScheme::Hanyu as i32,
            )),
            use_tone: Arc::new(std::sync::atomic::AtomicBool::new(
                bits.contains(oxpinyin_core::USE_TONE),
            )),
            force_tone: Arc::new(std::sync::atomic::AtomicBool::new(
                bits.contains(oxpinyin_core::FORCE_TONE),
            )),
            options: Arc::new(std::sync::atomic::AtomicU32::new(option_word)),
        }
    }
}

/// State behind a facade's context handle, minus the C parts: the shared
/// assembly, the user-learning store, the layered configuration, and the
/// live option/scheme word every allocated instance shares.
pub struct ContextCore {
    /// The layered configuration instances are opened with (the pinned
    /// upstream defaults).
    pub config: Config,
    /// The shared concrete assembly; `None` under a user-store-only
    /// context.
    pub runtime: Option<Runtime>,
    /// The user-learning store, shared by value-clone with every
    /// instance. `None` when the caller passed no usable user directory —
    /// an unusable dir must not make init fail; training degrades to
    /// refusing, upstream-style.
    pub user: Option<UserStore>,
    /// The live option/scheme word, shared with every instance.
    pub live: LiveOptions,
}

impl ContextCore {
    /// Opens a context the way an init does: system tables plus the
    /// optional user dir, health-checked, with `option_word` as the
    /// seeding word (per-facade: `PINYIN_DEFAULT_OPTION_WORD` /
    /// `ZHUYIN_DEFAULT_OPTION_WORD`).
    ///
    /// `None` is the C init's NULL: an empty system dir or a runtime
    /// that cannot open.
    #[must_use]
    pub fn open(system_dir: &str, user_dir: &str, option_word: u32) -> Option<Self> {
        if system_dir.is_empty() {
            return None;
        }
        let runtime = Runtime::open(Path::new(system_dir), Some(Path::new(user_dir))).ok()?;
        let user = runtime.user_store();
        Some(Self {
            config: Config::default(),
            runtime: Some(runtime),
            user,
            live: LiveOptions::new(option_word),
        })
    }

    /// User-store-only context for standalone tools (the §9 import/export
    /// machinery): a decoder context this is not, so
    /// [`ContextCore::alloc_instance`] answers `None` for it.
    #[must_use]
    pub fn new_user_only(user_dir: &str, option_word: u32) -> Option<Self> {
        if user_dir.is_empty() {
            return None;
        }
        let user = UserStore::open(&Path::new(user_dir).join(user_store_file())).ok()?;
        Some(Self {
            config: Config::default(),
            runtime: None,
            user: Some(user),
            live: LiveOptions::new(option_word),
        })
    }

    /// `set_options`'s law: the word is stored, the bools it carries are
    /// mirrored into their live flags, and the `incomplete-pinyin`
    /// configuration key follows the word so sessions opened later agree
    /// with sessions already allocated.
    pub fn set_options(&mut self, options: u32) {
        let enabled = (options & oxpinyin_core::PINYIN_INCOMPLETE) != 0;
        let use_tone = (options & oxpinyin_core::USE_TONE) != 0;
        let force_tone = (options & oxpinyin_core::FORCE_TONE) != 0;
        self.config
            .set("incomplete-pinyin", ConfigValue::Bool(enabled));
        let live = &self.live;
        live.incomplete
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
        live.use_tone
            .store(use_tone, std::sync::atomic::Ordering::Relaxed);
        live.force_tone
            .store(force_tone, std::sync::atomic::Ordering::Relaxed);
        live.options
            .store(options, std::sync::atomic::Ordering::Relaxed);
    }

    /// Allocates one instance's orchestration state over this context's
    /// assembly. `None` without a runtime (nothing to decode with).
    #[must_use]
    pub fn alloc_instance(&self) -> Option<crate::instance::InstanceCore> {
        let runtime = self.runtime.as_ref()?;
        let session = runtime.new_session(&self.config).ok()?;
        Some(crate::instance::InstanceCore::new(
            session,
            self.user.clone(),
            runtime.dict(),
            runtime.lm(),
            self.live.clone(),
        ))
    }

    /// `save`'s body: `false` without a user dir, otherwise the store's
    /// gated save — `false` when unmodified, `true` after a dirty save.
    pub fn save_user(&mut self) -> bool {
        self.user
            .as_mut()
            .is_some_and(|store| store.save().unwrap_or(false))
    }

    /// `mask_out`'s body: the store-level deletion, or `false` without a
    /// user store.
    pub fn mask_out(&mut self, mask: u32, value: u32) -> bool {
        self.user
            .as_mut()
            .is_some_and(|store| store.mask_out(mask, value).is_ok())
    }

    /// `load_phrase_library`'s read side: the runtime's library-load
    /// (mask-clear) rule; `false` without a runtime.
    #[must_use]
    pub fn load_phrase_library(&self, index: u32) -> bool {
        self.runtime
            .as_ref()
            .is_some_and(|runtime| runtime.load_library(index))
    }

    /// `unload_phrase_library`'s read side; `false` without a runtime.
    #[must_use]
    pub fn unload_phrase_library(&self, index: u8) -> bool {
        self.runtime
            .as_ref()
            .is_some_and(|runtime| runtime.unload_library(u32::from(index)))
    }

    /// Clone of the context's user store, if this context has one.
    #[must_use]
    pub fn user_store(&self) -> Option<UserStore> {
        self.user.clone()
    }
}
