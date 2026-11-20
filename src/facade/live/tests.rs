use super::LiveContexts;

#[test]
fn poisoned_mutex_aborts() {
    crate::store::death_test::assert_aborts_after_setup(
        "facade::live::tests::poisoned_mutex_aborts",
        || {
            let live = LiveContexts::new();
            let panic = std::panic::catch_unwind(|| {
                let _guard = live.lock();
                panic!("poison the mutex");
            });
            assert!(panic.is_err());
            assert!(live.addresses.is_poisoned());
            live
        },
        |live| live.register(8),
    );
}

#[test]
fn the_registry_holds_no_allocation_once_it_empties() {
    let live = LiveContexts::new();
    live.register(8);
    live.register(16);
    live.register(8);
    assert_eq!(live.lock().len(), 2, "an address is held once");
    assert!(live.unregister(8));
    assert!(live.contains(16));
    assert!(live.unregister(16));
    assert_eq!(live.lock().capacity(), 0);
    assert!(!live.unregister(16));
    assert_eq!(live.lock().capacity(), 0);
    live.register(8);
    assert!(live.contains(8));
}
