use super::merge_bigram;

// 074a2219 ngram.cpp:305,332 merges present counts at their stored width.
#[test]
fn bigram_merge_wraps_each_field_at_the_stored_width() {
    for (system, user, expected) in [
        ((2, 0), 0, (2, 0)),
        (
            (1, 1),
            u32::MAX - 1,
            (u64::from(u32::MAX), u64::from(u32::MAX)),
        ),
        ((1, 1), u32::MAX, (0, 0)),
    ] {
        assert_eq!(
            merge_bigram(Some(system), user.into(), user.into()),
            Some(expected)
        );
    }
}

mod language_model {
    use std::cell::Cell;

    use crate::model::{LanguageModel, UserCountDelta};

    use crate::data::{
        BigramLanguageModel, BigramTables, PhraseLibraries, SYSTEM_LIBRARY_FILES, SystemDbm,
    };

    use crate::store::{DefaultStore, WriteStore};

    use crate::data::support::ChunkBuilder;

    use tempfile::TempDir;

    /// The n-best cost law for one row item `(freq, total)`; `None` leaves
    /// the unigram branch alone.
    fn item_costs(
        lm: &BigramLanguageModel<'_>,
        token: u32,
        user: UserCountDelta,
        item: Option<(u32, u32)>,
    ) -> crate::model::NbestStepCosts {
        lm.nbest_token_costs_with(&token, user)
            .costs(item.map(|(freq, total)| (u64::from(freq), u64::from(total), true)))
    }

    const NI: u32 = 0x0100_0010;

    const HAO: u32 = 0x0100_0011;

    const NIHAO: u32 = 0x0100_0099;

    const GBK: u32 = 0x0200_0001;

    struct ModelFixture {
        tables: BigramTables,
        libraries: PhraseLibraries,
        mask: Cell<u32>,
    }

    impl ModelFixture {
        fn model(&self) -> BigramLanguageModel<'_> {
            self.tables.read(&self.libraries, &self.mask)
        }
    }

    struct Fixture {
        dir: TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::Builder::new()
                .prefix("oxpinyin-lm-test-")
                .tempdir()
                .unwrap();
            let mut gb = ChunkBuilder::new(16);
            gb.add(0x10, 10, "你", vec![(vec![0, 0], 1)]);
            gb.add(0x11, 5, "好", vec![(vec![0, 0], 1)]);
            gb.add(0x99, 1, "你好", vec![(vec![0, 0, 0, 0], 1)]);
            std::fs::write(dir.path().join("gb_char.bin"), gb.build()).unwrap();
            let mut gbk = ChunkBuilder::new(3);
            gbk.add(0x01, 3, "鎄", vec![(vec![0, 0], 1)]);
            std::fs::write(dir.path().join("gbk_char.bin"), gbk.build()).unwrap();
            let store =
                DefaultStore::create_hash(&dir.path().join(SystemDbm::Bigram.file_name())).unwrap();
            let mut value = 7_u32.to_le_bytes().to_vec();
            value.extend_from_slice(&HAO.to_le_bytes());
            value.extend_from_slice(&7_u32.to_le_bytes());
            store.set_raw(&NI.to_le_bytes(), &value).unwrap();
            Self { dir }
        }

        fn open(&self) -> ModelFixture {
            let libraries = PhraseLibraries::open(self.dir.path(), SYSTEM_LIBRARY_FILES).unwrap();
            let path = self.dir.path().join(SystemDbm::Bigram.file_name());
            ModelFixture {
                tables: BigramTables::attach(&path, 0.312_699).unwrap(),
                libraries,
                mask: Cell::new(0),
            }
        }
    }

    // Rust BigramLanguageModel reads the visible item fields and facade total.
    #[test]
    fn unigrams_are_the_chunk_item_fields() {
        let fix = Fixture::new();
        let model = fix.open();
        let lm = model.model();
        assert_eq!(
            lm.unigram_count(NI),
            Some(10),
            "\\1-gram 9 + gen_unigram's 1"
        );
        assert_eq!(
            lm.unigram_count(NIHAO),
            Some(1),
            "never seen: still 1, never 0"
        );
        assert_eq!(lm.unigram_count(GBK), Some(3));
        assert_eq!(lm.unigram_count(0x0300_0001), None, "no such library");
        assert_eq!(lm.unigram_total(), 16 + 3, "Σ item over both chunks");
        assert!(lm.has_unigrams());
        assert_eq!(LanguageModel::unigram_freq(&lm, &NI).unwrap(), Some(10));
        assert_eq!(LanguageModel::unigram_total(&lm).unwrap(), Some(19));
    }

    // Rust BigramLanguageModel excludes masked libraries from both counts and totals.
    #[test]
    fn an_unloaded_library_leaves_the_model() {
        let fix = Fixture::new();
        let model = fix.open();
        let lm = model.model();
        model.mask.set(1 << 2);
        assert_eq!(lm.unigram_count(GBK), None);
        assert_eq!(lm.unigram_total(), 16, "gbk's total leaves");
        assert_eq!(lm.unigram_count(NI), Some(10));
    }

    // 074a2219 phonetic_lookup.h:646-693 prices seen and unseen transitions from corpus counts.
    #[test]
    fn nbest_steps_blend_bigram_and_unigram_over_the_corpus_counts() {
        let fix = Fixture::new();
        let model = fix.open();
        let lm = model.model();
        let ni = NI;
        let hao = HAO;
        let nihao = NIHAO;
        let seen = lm.nbest_step_costs(&ni, &hao).unwrap();
        assert!(seen.blended.is_some() && seen.unigram.is_some());
        assert!(seen.blended.unwrap() > seen.unigram.unwrap() * f64::from(seen.unigram_lambda));
        let unseen = lm.nbest_step_costs(&hao, &ni).unwrap();
        assert!(unseen.blended.is_none());
        assert!(unseen.unigram.is_some());
        let rare = lm.nbest_step_costs(&hao, &nihao).unwrap();
        assert!(rare.unigram.is_some());
        assert!(
            rare.unigram.unwrap() < unseen.unigram.unwrap(),
            "rarer than 你"
        );
        assert_eq!(
            lm.nbest_step_costs(&hao, &0x0300_0001).unwrap(),
            crate::model::NbestStepCosts::default()
        );
        let raised = lm
            .nbest_step_costs_with_user_delta(
                &hao,
                &nihao,
                UserCountDelta {
                    unigram_delta: 3,
                    unigram_total_delta: 3,
                    ..UserCountDelta::ZERO
                },
            )
            .unwrap();
        assert!(raised.unigram.unwrap() > rare.unigram.unwrap());
    }

    // 074a2219 ngram.cpp:231-250,305 distinguishes a stored zero from a missing successor.
    #[test]
    fn wrapped_bigram_fields_keep_record_presence_and_nan_costs() {
        let fix = Fixture::new();
        let model = fix.open();
        let lm = model.model();
        let ni = NI;
        let hao = HAO;
        let delta = UserCountDelta {
            bigram_count: u64::from(u32::MAX - 6),
            bigram_total: u64::from(u32::MAX - 6),
            ..UserCountDelta::ZERO
        };
        assert!(
            lm.nbest_step_costs_with_user_delta(&ni, &hao, delta)
                .unwrap()
                .blended
                .unwrap()
                .is_nan()
        );
        let delta = UserCountDelta {
            bigram_count: u64::from(u32::MAX),
            bigram_total: 0,
            ..UserCountDelta::ZERO
        };
        assert_eq!(
            lm.nbest_step_costs_with_user_delta(&hao, &ni, delta)
                .unwrap()
                .blended,
            Some(f64::INFINITY)
        );
        let absent = UserCountDelta {
            bigram_count: 0,
            ..delta
        };
        assert_eq!(
            lm.nbest_step_costs_with_user_delta(
                &ni,
                &NIHAO,
                UserCountDelta {
                    bigram_total: u64::from(u32::MAX - 6),
                    ..absent
                }
            )
            .unwrap()
            .blended,
            None
        );
    }

    fn merged_row_costs(
        lm: &BigramLanguageModel<'_>,
        prev: u32,
        token: u32,
        user: UserCountDelta,
        user_gram: (u64, Vec<(u32, u64)>),
    ) -> crate::model::NbestStepCosts {
        use crate::model::{MergedGram, NbestStepRow};
        let bytes = MergedGram::new(user_gram.0, user_gram.1).to_bytes();
        let mut row = NbestStepRow::default();
        let gram = row
            .get_or_try_init(|| lm.load_merged_row(prev, Some(bytes)))
            .unwrap()
            .expect("the user gram is present");
        let item = gram
            .get_freq(token)
            .unwrap()
            .map(|freq| (freq, gram.total().unwrap()));
        item_costs(lm, token, user, item)
    }

    // 074a2219 phonetic_lookup.h:605-608,692-693 preserves mixed precision above one.
    #[test]
    fn an_above_one_blend_keeps_the_pins_mixed_precision_rounding() {
        let fix = Fixture::new();
        let model = fix.open();
        let lm = model.model();
        let ni = NI;
        let hao = HAO;
        let delta = UserCountDelta {
            bigram_count: 2_134_351_507 - 7,
            bigram_total: 602_365_351 - 7,
            unigram_delta: 21_950_320 - 5,
            unigram_total_delta: 4_294_967_295 - 19,
            unigram_live: None,
        };
        let pair = lm
            .nbest_step_costs_with_user_delta(&ni, &hao, delta)
            .unwrap();
        assert_eq!(pair.blended, Some(1.1114937993141745));

        let raw = merged_row_costs(
            &lm,
            NI,
            HAO,
            delta,
            (delta.bigram_total, vec![(HAO, delta.bigram_count)]),
        );
        assert_eq!(raw.blended, Some(1.1114937993141745));
        assert_eq!(raw.unigram, pair.unigram);
    }

    // 074a2219 phrase_index.h:440 stores the facade total as guint32.
    #[test]
    fn the_nbest_denominator_wraps_as_the_facade_total() {
        let fix = Fixture::new();
        let model = fix.open();
        let lm = model.model();
        let ni = NI;
        let hao = HAO;
        assert_eq!(lm.facade_total(0), 19);
        assert_eq!(lm.facade_total(u64::from(u32::MAX)), 18);
        let wrapped = UserCountDelta {
            unigram_total_delta: 1 << 32,
            ..UserCountDelta::ZERO
        };
        assert_eq!(
            lm.nbest_step_costs_with_user_delta(&ni, &hao, wrapped)
                .unwrap(),
            lm.nbest_step_costs_with_user_delta(&ni, &hao, UserCountDelta::ZERO)
                .unwrap()
        );
    }

    // Rust has_unigrams tests visible items even when the total wraps to zero.
    #[test]
    fn unigram_availability_survives_a_total_that_wraps_to_zero() {
        let fix = Fixture::new();
        let mut gb = ChunkBuilder::new(u32::MAX - 2);
        gb.add(0x10, 10, "你", vec![(vec![0, 0], 1)]);
        gb.add(0x11, 5, "好", vec![(vec![0, 0], 1)]);
        gb.add(0x99, 1, "你好", vec![(vec![0, 0, 0, 0], 1)]);
        std::fs::write(fix.dir.path().join("gb_char.bin"), gb.build()).unwrap();
        let model = fix.open();
        let lm = model.model();
        assert_eq!(
            lm.unigram_total(),
            0,
            "gb_char's total plus gbk_char's 3 wraps"
        );
        assert!(lm.has_unigrams());
        model.mask.set((1 << 1) | (1 << 2));
        assert!(!lm.has_unigrams(), "no visible library holds an item");
    }

    // 074a2219 phrase_lookup.cpp:310,329 requires a readable phrase item before pricing.
    #[test]
    fn user_file_tokens_price_only_when_their_item_is_present() {
        let fix = Fixture::new();
        let model = fix.open();
        let lm = model.model();
        let start = 1;
        let item = |unigram: u32| UserCountDelta {
            unigram_delta: u64::from(unigram),
            unigram_total_delta: u64::from(unigram),
            unigram_live: Some(unigram),
            ..UserCountDelta::ZERO
        };

        for absent in [
            UserCountDelta::ZERO,
            UserCountDelta {
                unigram_delta: 27,
                unigram_total_delta: 27,
                ..UserCountDelta::ZERO
            },
        ] {
            let step = lm
                .nbest_step_costs_with_user_delta(&start, &0x0700_0002, absent)
                .unwrap();
            assert_eq!(step, crate::model::NbestStepCosts::default());
        }

        let priced = lm
            .nbest_step_costs_with_user_delta(&start, &0x0700_0002, item(27))
            .unwrap();
        assert!(priced.unigram.is_some(), "a present item must price");
        assert!(priced.blended.is_none(), "no bigram row involves it");

        let zero_with_bigram = lm
            .nbest_step_costs_with_user_delta(
                &NI,
                &0x0700_0002,
                UserCountDelta {
                    bigram_count: 5,
                    bigram_total: 5,
                    unigram_live: Some(0),
                    ..UserCountDelta::ZERO
                },
            )
            .unwrap();
        assert!(zero_with_bigram.blended.is_some());
        assert!(zero_with_bigram.unigram.is_none());

        let stale_with_bigram = lm
            .nbest_step_costs_with_user_delta(
                &NI,
                &0x0700_0002,
                UserCountDelta {
                    bigram_count: 5,
                    bigram_total: 5,
                    ..UserCountDelta::ZERO
                },
            )
            .unwrap();
        assert_eq!(stale_with_bigram, crate::model::NbestStepCosts::default());

        let masked_model = fix.open();
        let lm2 = masked_model.model();
        masked_model.mask.set(1 << 7);
        let masked = lm2
            .nbest_step_costs_with_user_delta(&start, &0x0700_0002, item(27))
            .unwrap();
        assert_eq!(masked, crate::model::NbestStepCosts::default());
        let fresh_model = fix.open();
        let lm3 = fresh_model.model();
        let delta = UserCountDelta {
            unigram_delta: 27,
            unigram_total_delta: 27,
            ..UserCountDelta::ZERO
        };
        for token in [0x0100_0050_u32, 0x0300_0001] {
            let step = lm3
                .nbest_step_costs_with_user_delta(&start, &token, delta)
                .unwrap();
            assert_eq!(
                step,
                crate::model::NbestStepCosts::default(),
                "nibble {} without an item keeps the default answer",
                token >> 24
            );
        }
    }

    fn conf_with_lambda(lambda: &str) -> crate::data::TableConf {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("table.conf"),
            format!(
                "binary format version:7\nmodel data version:12\nlambda parameter:{lambda}\n\
             source table format:pinyin\ndatabase format:BerkeleyDB\n"
            ),
        )
        .unwrap();
        crate::data::TableConf::load_from_dir(dir.path()).unwrap()
    }

    // Rust row and pair pricing agree for merged counts; only the row represents an explicit user zero.
    #[test]
    fn merged_row_items_price_as_the_pair_overlay() {
        use crate::model::{MergedGram, NbestStepRow};
        let fix = Fixture::new();
        let model = fix.open();
        let lm = model.model();
        let user_rows = [
            (NI, u64::from(u32::MAX)),
            (HAO, u64::from(u32::MAX - 6)),
            (NIHAO, 0),
        ];
        for (prev, total, token, bigram_count) in [
            (NI, u64::from(u32::MAX - 6), HAO, u64::from(u32::MAX - 6)),
            (HAO, 17, NI, u64::from(u32::MAX)),
            (NI, 17, NIHAO, 0),
            (HAO, 17, GBK, 0),
        ] {
            let user_gram = MergedGram::new(total, user_rows.to_vec()).to_bytes();
            let mut row = NbestStepRow::default();
            let user = UserCountDelta {
                bigram_count,
                bigram_total: total,
                unigram_delta: 3,
                unigram_total_delta: 9,
                unigram_live: None,
            };
            let expected = lm
                .nbest_step_costs_with_user_delta(&prev, &token, user)
                .unwrap();
            let gram = row
                .get_or_try_init(|| lm.load_merged_row(prev, Some(user_gram.clone())))
                .unwrap()
                .expect("the user gram is present");
            let item = gram
                .get_freq(token)
                .unwrap()
                .map(|freq| (freq, gram.total().unwrap()));
            let actual = item_costs(&lm, token, user, item);
            if token == NIHAO {
                assert_eq!(item.map(|(freq, _)| freq), Some(0), "the stored zero stays");
                assert_eq!(actual.unigram, expected.unigram);
                assert!(actual.blended.is_some(), "prev={prev} total={total}");
            } else {
                assert_eq!(
                    actual.blended.map(f64::to_bits),
                    expected.blended.map(f64::to_bits),
                    "prev={prev} token={token} total={total}"
                );
                assert_eq!(
                    actual.unigram.map(f64::to_bits),
                    expected.unigram.map(f64::to_bits)
                );
                assert_eq!(
                    actual.unigram_lambda.to_bits(),
                    expected.unigram_lambda.to_bits()
                );
            }
        }
    }

    // Rust raw loading retains a present empty gram while checked loading refuses it.
    #[test]
    fn reusable_row_keeps_visibility_live_and_reads_an_empty_value_as_stored() {
        use crate::model::NbestStepRow;
        let fix = Fixture::new();
        let store =
            DefaultStore::create_hash(&fix.dir.path().join(SystemDbm::Bigram.file_name())).unwrap();
        store.set_raw(&NI.to_le_bytes(), &[]).unwrap();
        drop(store);
        let model = fix.open();
        let lm = model.model();
        let mut row = NbestStepRow::default();
        let prev = NI;
        model.mask.set(1 << 2);
        assert_eq!(
            lm.nbest_token_costs(&GBK)
                .unwrap()
                .costs(Some((1, 1, true))),
            crate::model::NbestStepCosts::default()
        );
        let Some(crate::model::NbestSuccessors::Row(gram)) =
            lm.nbest_successors_in_row(&prev, &mut row).unwrap()
        else {
            panic!("a present empty value is a row");
        };
        assert!(gram.to_bytes().is_empty());
        let Err(error) = lm.nbest_step_costs(&prev, &HAO) else {
            panic!("the pair path refuses a present empty value");
        };
        assert!(matches!(error, crate::data::LmError::Parse));
    }

    // 074a2219 ngram.cpp:231-250 searches the stored row without reordering.
    #[test]
    fn row_and_pair_read_a_stored_unsorted_row_through_the_pins_lower_bound() {
        use crate::model::NbestSuccessors;
        let fix = Fixture::new();
        let store =
            DefaultStore::create_hash(&fix.dir.path().join(SystemDbm::Bigram.file_name())).unwrap();
        let value =
            crate::single_gram::encode_single_gram(11, &[(NIHAO, 0), (HAO, 7), (NI, 4), (HAO, 99)]);
        store.set_raw(&NI.to_le_bytes(), &value).unwrap();
        drop(store);
        let model = fix.open();
        let lm = model.model();
        let mut row = crate::model::NbestStepRow::default();
        let prev = NI;
        let Some(NbestSuccessors::Row(gram)) = lm.nbest_successors_in_row(&prev, &mut row).unwrap()
        else {
            panic!("the system row is present");
        };
        assert_eq!(gram.to_bytes(), value.as_slice());
        let freq = gram.get_freq(HAO).unwrap();
        assert_eq!(freq, Some(99));
        let item = freq.map(|freq| (freq, gram.total().unwrap()));
        assert_eq!(
            lm.nbest_token_costs(&HAO)
                .unwrap()
                .costs(item.map(|(freq, total)| (u64::from(freq), u64::from(total), true))),
            lm.nbest_step_costs(&prev, &HAO).unwrap()
        );
    }

    // 074a2219 phonetic_lookup.h:646-693 applies epsilon gates before weighting.
    #[test]
    fn prelog_branches_preserve_ieee_lambda_edges_and_epsilon_gates() {
        let fix = Fixture::new();
        let mut model = fix.open();
        for text in ["-0.5", "0", "1", "1.5", "inf", "nan"] {
            let conf = conf_with_lambda(text);
            let lambda = conf.lambda_f32();
            model.tables.lambda_f32 = lambda;
            let lm = model.model();
            let branches = item_costs(&lm, HAO, UserCountDelta::ZERO, Some((7, 7)));
            // The engine steps each branch from the token's costs alone.
            let token = lm.nbest_token_costs_with(&HAO, UserCountDelta::ZERO);
            assert_eq!(
                token.blended(7, 7).map(f64::to_bits),
                branches.blended.map(f64::to_bits),
                "{text}"
            );
            assert_eq!(token.unigram(), branches.unigram, "{text}");
            assert_eq!(
                token.unigram_lambda.to_bits(),
                branches.unigram_lambda.to_bits()
            );
            assert_eq!(branches.unigram, Some(5.0 / 19.0));
            let unilambda = (1.0_f64 - f64::from(lambda)) as f32;
            assert_eq!(branches.unigram_lambda.to_bits(), unilambda.to_bits());
            let expected = f64::from(lambda * 1.0_f32) + f64::from(unilambda) * (5.0 / 19.0);
            let actual = branches.blended.unwrap();
            if expected.is_nan() {
                assert!(actual.is_nan(), "{text}");
            } else {
                assert_eq!(actual.to_bits(), expected.to_bits(), "{text}");
            }
        }
        model.tables.lambda_f32 = conf_with_lambda("0.5").lambda_f32();
        let lm = model.model();
        let zero = 0x0700_0002;
        let zero_item = UserCountDelta {
            unigram_live: Some(0),
            ..UserCountDelta::ZERO
        };
        for (total, present) in [(1 << 24, false), (1 << 23, true)] {
            let step = item_costs(&lm, zero, zero_item, Some((1, total)));
            assert!(step.unigram.is_none());
            assert_eq!(step.blended.is_some(), present);
            let token = lm.nbest_token_costs_with(&zero, zero_item);
            assert!(token.unigram().is_none());
            assert_eq!(token.blended(1, u64::from(total)), step.blended);
        }
        assert!(
            item_costs(&lm, zero, zero_item, Some((0, 0)))
                .blended
                .unwrap()
                .is_nan()
        );
        // A token without an item answers the default and reads no
        // transition; neither branch steps from its costs.
        let absent = lm.nbest_token_costs_with(&0x0300_0001, UserCountDelta::ZERO);
        assert_eq!(absent, crate::model::NbestTokenCosts::ABSENT);
        assert_eq!((absent.blended(1, 1), absent.unigram()), (None, None));
        assert_eq!(
            lm.nbest_step_costs_using(&0x0300_0001, UserCountDelta::ZERO, || {
                panic!("no transition is read without an item")
            })
            .unwrap(),
            crate::model::NbestStepCosts::default()
        );
    }
}
