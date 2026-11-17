use super::merge_two_phrase_array;
use crate::k_mixture_model::records::{ArrayItemWithToken, KMixtureModelArrayItem};

fn entry(token: u32, wc: u32, mr: u32) -> ArrayItemWithToken<KMixtureModelArrayItem> {
    ArrayItemWithToken {
        token,
        item: KMixtureModelArrayItem {
            m_wc: wc,
            m_n_n_0: wc,
            m_n_1: 0,
            m_mr: mr,
        },
    }
}

#[test]
fn matching_tokens_sum_and_max_and_others_keep_their_order() {
    let first = vec![entry(1, 1, 1), entry(3, 2, 2), entry(7, 4, 4)];
    let second = vec![entry(2, 5, 5), entry(3, 6, 9), entry(8, 1, 1)];
    let merged = merge_two_phrase_array(&first, &second);
    let tokens: Vec<u32> = merged.iter().map(|entry| entry.token).collect();
    assert_eq!(tokens, [1, 2, 3, 7, 8]);
    let three = &merged[2].item;
    assert_eq!(three.m_wc, 8);
    assert_eq!(three.m_n_n_0, 8);
    assert_eq!(three.m_n_1, 0);
    assert_eq!(three.m_mr, 9);
}

/// A short target row with a whole source row: the pin loads both
/// (`:142-143`) and asserts at `check_result(target->get_array_header(…))`
/// (`:156`).
#[test]
fn a_short_target_row_aborts_at_the_targets_header() {
    use crate::k_mixture_model::database::tests::{
        SHORT, TOKEN, WHOLE, assert_aborts, child_dir, plant,
    };
    if let Some(dir) = child_dir() {
        let target = plant(&dir, "target.db", &[(TOKEN, SHORT)]);
        let source = plant(&dir, "source.db", &[(TOKEN, WHOLE)]);
        let _ = super::merge_array_items(
            &crate::k_mixture_model::database::Database::open_read_write(Some(&target)),
            &crate::k_mixture_model::database::Database::open_read_only(Some(&source)),
        );
        return;
    }
    assert_aborts("k_mixture_model::merge::tests::a_short_target_row_aborts_at_the_targets_header");
}

/// A short source row that the target lacks: the pin stores the gram it
/// loaded, its bytes unchanged (`:142-146`), and the merge goes on.
#[test]
fn a_short_source_row_the_target_lacks_is_copied_unchanged() {
    use crate::k_mixture_model::database::tests::{SHORT, TOKEN, plant};
    use oxpinyin_store::{
        AttachMode, DefaultStore, HashAttach, MAGIC_KEY, RawReadStore as _, WriteStore as _,
    };
    let dir = tempfile::tempdir().expect("scratch directory");
    let target = plant(dir.path(), "target.db", &[]);
    let source = plant(dir.path(), "source.db", &[(TOKEN, SHORT)]);
    {
        let target = crate::k_mixture_model::database::Database::open_read_write(Some(&target));
        let source = crate::k_mixture_model::database::Database::open_read_only(Some(&source));
        assert!(super::merge_array_items(&target, &source));
    }
    let HashAttach::Opened(db) = DefaultStore::attach_hash(
        &dir.path().join("target.db"),
        AttachMode::ReadOnly,
        (&MAGIC_KEY, b"KMMP"),
    )
    .expect("reopen the target") else {
        panic!("the target opens");
    };
    let mut value = Vec::new();
    assert!(db.get_raw(&TOKEN.to_ne_bytes(), &mut value).expect("read"));
    assert_eq!(value, SHORT);
}
