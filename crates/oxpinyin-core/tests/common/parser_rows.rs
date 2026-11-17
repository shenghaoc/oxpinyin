// Frozen option-row cases, shared by the parser, facade and C text tests.
/// The parser-table rows one option bit gates, each with its bit on, then with every other gate
/// bit on and its own off. A gated row is admitted only when its flag is in the options
/// (`storage/pinyin_parser2.cpp:49-55` at 074a2219); it then parses to its own key
/// (`:143-147`), and the DP prefers the fewest keys, then the least distance (`:298-309`).
/// With the row refused, PINYIN_INCOMPLETE (in the base word) lets an initial stand alone
/// (`:43-44`). The auxiliary text renders the keys at the parsed length
/// (`pinyin.cpp:3381-3437`). Pin capture of 2026-11-07, runs 1 and 2 identical, observations
/// `option-row/on/<input>`, `option-row/off/<input>` and `option-distance/chonga`.
macro_rules! gated_parser_rows {
    ($test:item) => {
        /// `OPTION_BASE` of `tools/oracle/pin_capture.py`: PINYIN_INCOMPLETE | USE_DIVIDED_TABLE |
        /// USE_RESPLIT_TABLE, the base word of the option cases (`pinyin_set_options`, `pinyin.cpp:1299-1307` at 074a2219).
        const OPTION_BASE: u32 = 0x188;

        /// PINYIN_CORRECT_ALL | PINYIN_AMB_S_SH | PINYIN_AMB_Z_ZH (`pinyin_custom2.h:51-52,79`).
        const OPTION_GATES: u32 = 0x1FE0_1800;

        #[rstest::rstest]
        #[case::gn_ng_on("agn", OPTION_BASE | 1 << 21, 3, &[(512, 0, 3, "ang")], "ang |")]
        #[case::gn_ng_off(
            "agn",
            (OPTION_BASE | OPTION_GATES) & !(1 << 21),
            3,
            &[(128, 0, 1, "a"), (7, 1, 2, "g"), (11, 2, 3, "n")],
            "a g n |"
        )]
        #[case::gn_ng_target_outside_the_inventory("egn", OPTION_BASE | 1 << 21, 3, &[(1280, 0, 3, "eng")], "eng |")]
        #[case::mg_ng_on("amg", OPTION_BASE | 1 << 22, 3, &[(512, 0, 3, "ang")], "ang |")]
        #[case::mg_ng_off(
            "amg",
            (OPTION_BASE | OPTION_GATES) & !(1 << 22),
            3,
            &[(128, 0, 1, "a"), (10, 1, 2, "m"), (7, 2, 3, "g")],
            "a m g |"
        )]
        #[case::iou_iu_on("diou", OPTION_BASE | 1 << 23, 4, &[(1956, 0, 4, "diu")], "diu |")]
        #[case::iou_iu_off(
            "diou",
            (OPTION_BASE | OPTION_GATES) & !(1 << 23),
            4,
            &[(36, 0, 2, "di"), (1920, 2, 4, "ou")],
            "di ou |"
        )]
        #[case::uei_ui_on("duei", OPTION_BASE | 1 << 24, 4, &[(1092, 0, 4, "dui")], "dui |")]
        #[case::uei_ui_off(
            "duei",
            (OPTION_BASE | OPTION_GATES) & !(1 << 24),
            4,
            &[(68, 0, 2, "du"), (1024, 2, 4, "ei")],
            "du ei |"
        )]
        #[case::uen_un_on("duen", OPTION_BASE | 1 << 25, 4, &[(1220, 0, 4, "dun")], "dun |")]
        #[case::uen_un_off(
            "duen",
            (OPTION_BASE | OPTION_GATES) & !(1 << 25),
            4,
            &[(68, 0, 2, "du"), (1152, 2, 4, "en")],
            "du en |"
        )]
        #[case::ue_ve_on("lue", OPTION_BASE | 1 << 26, 3, &[(876, 0, 3, "lve")], "lve |")]
        #[case::ue_ve_off(
            "lue",
            (OPTION_BASE | OPTION_GATES) & !(1 << 26),
            3,
            &[(76, 0, 2, "lu"), (768, 2, 3, "e")],
            "lu e |"
        )]
        #[case::v_u_on("jv", OPTION_BASE | 1 << 27, 2, &[(105, 0, 2, "ju")], "ju |")]
        #[case::v_u_off("jv", (OPTION_BASE | OPTION_GATES) & !(1 << 27), 1, &[(9, 0, 1, "j")], "j |")]
        #[case::on_ong_on("zon", OPTION_BASE | 1 << 28, 3, &[(1814, 0, 3, "zong")], "zong |")]
        #[case::on_ong_off(
            "zon",
            (OPTION_BASE | OPTION_GATES) & !(1 << 28),
            3,
            &[(22, 0, 1, "z"), (1664, 1, 2, "o"), (11, 2, 3, "n")],
            "z o n |"
        )]
        #[case::s_sh_on("sua", OPTION_BASE | 1 << 11, 3, &[(209, 0, 3, "shua")], "shua |")]
        #[case::s_sh_off(
            "sua",
            (OPTION_BASE | OPTION_GATES) & !(1 << 11),
            3,
            &[(80, 0, 2, "su"), (128, 2, 3, "a")],
            "su a |"
        )]
        #[case::z_zh_on("zua", OPTION_BASE | 1 << 12, 3, &[(215, 0, 3, "zhua")], "zhua |")]
        #[case::z_zh_off(
            "zua",
            (OPTION_BASE | OPTION_GATES) & !(1 << 12),
            3,
            &[(86, 0, 2, "zu"), (128, 2, 3, "a")],
            "zu a |"
        )]
        #[case::distance_tie(
            "chonga",
            OPTION_BASE | 1 << 28,
            6,
            &[(1795, 0, 5, "chong"), (128, 5, 6, "a")],
            "chong a |"
        )]
        $test
    };
}
pub(crate) use gated_parser_rows;
