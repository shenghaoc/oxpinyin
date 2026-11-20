use super::*;
use crate::test_support::{TempUserDir, candidate, finalize_public_context, open_at};

#[test]
fn handles_retain_context_and_borrow_the_live_backing() {
    let user = TempUserDir::new("state-handles");
    let (context, instance) = open_at(&user.path);

    // SAFETY: the fixture created both handles. Each borrow ends before the
    // next mutable access. The candidate stays in the current snapshot.
    unsafe {
        context_mut(context).core.set_options(OptionBits::USE_TONE);
        assert_eq!(
            context_ref(context).core.live.options.get(),
            OptionBits::USE_TONE
        );
        let row = candidate(instance, "nihao", 0);
        let view = instance_ref(instance);
        let first = view.candidates.iter().next().unwrap();
        assert_eq!(candidate_ptr(first), row);
        assert!(std::ptr::eq(candidate_ref(row), first));
        assert!(crate::sentence::pinyin_guess_sentence(instance));
        finalize_public_context(context, instance);
    }

    // SAFETY: the instance retains the context allocation after fini. All
    // calls are serial and the last borrow ends before the instance is freed.
    unsafe {
        instance_mut(instance).full_aux_options = OptionBits::USE_TONE;
        assert_eq!(
            instance_ref(instance).full_aux_options,
            OptionBits::USE_TONE
        );
        assert!(
            !owner_ref(instance_ref(instance).owner)
                .core
                .user_store()
                .is_modified()
        );
        assert!(crate::candidates::pinyin_train(instance, 0));
        assert!(
            owner_ref(instance_ref(instance).owner)
                .core
                .user_store()
                .is_modified()
        );
        crate::instance::pinyin_free_instance(instance);
    }
}
