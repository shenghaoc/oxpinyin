use super::LiveContexts;

// Rust contract: LiveContexts releases its allocation when the last address is removed.
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
