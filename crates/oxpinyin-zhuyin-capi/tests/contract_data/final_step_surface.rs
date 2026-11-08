//! Final-step sentences and candidate windows of short full-pinyin inputs on both facades (#1175, #594). Generated from the frozen pin observations.

use crate::bodies::final_step_surface as b;
use crate::support::{Expect, run_j};

// final-step-zhuyin-ba'ku-20: MATCH.
#[test]
fn final_step_zhuyin_ba_apos_ku_20() {
    run_j(
        "final_step_surface::final_step_zhuyin_ba_apos_ku_20",
        Expect::RESULT,
        |k| b::case(k, "final-step-zhuyin-ba'ku-20"),
        raw!(include!("fixtures/final_step_surface/4beb0ccb904816cf.rs")),
        None,
    );
}

// final-step-zhuyin-ba'kua-20: MATCH.
#[test]
fn final_step_zhuyin_ba_apos_kua_20() {
    run_j(
        "final_step_surface::final_step_zhuyin_ba_apos_kua_20",
        Expect::RESULT,
        |k| b::case(k, "final-step-zhuyin-ba'kua-20"),
        raw!(include!("fixtures/final_step_surface/1e24065e46154535.rs")),
        None,
    );
}

// final-step-zhuyin-import-ba-kua: MATCH.
#[test]
fn final_step_zhuyin_import_ba_kua() {
    run_j(
        "final_step_surface::final_step_zhuyin_import_ba_kua",
        Expect::RESULT,
        |k| b::case(k, "final-step-zhuyin-import-ba-kua"),
        raw!(include!("fixtures/final_step_surface/bcd22278639ae605.rs")),
        None,
    );
}

// final-step-zhuyin-li'shi-20: MATCH.
#[test]
fn final_step_zhuyin_li_apos_shi_20() {
    run_j(
        "final_step_surface::final_step_zhuyin_li_apos_shi_20",
        Expect::RESULT,
        |k| b::case(k, "final-step-zhuyin-li'shi-20"),
        raw!(include!("fixtures/final_step_surface/a921b9b6127fee85.rs")),
        None,
    );
}

// final-step-zhuyin-n-20: MATCH.
#[test]
fn final_step_zhuyin_n_20() {
    run_j(
        "final_step_surface::final_step_zhuyin_n_20",
        Expect::RESULT,
        |k| b::case(k, "final-step-zhuyin-n-20"),
        raw!(M(&[
            (
                "candidates",
                M(&[("after-cursor", M(&[("ret", B(false)), ("rows", L(&[]))]))])
            ),
            ("guessed", B(false)),
            ("logs", L(&[])),
            ("nbest_count", I(0)),
            ("parsed", I(0)),
            ("sentences", L(&[]))
        ])),
        None,
    );
}

// final-step-zhuyin-n-28: MATCH.
#[test]
fn final_step_zhuyin_n_28() {
    run_j(
        "final_step_surface::final_step_zhuyin_n_28",
        Expect::RESULT,
        |k| b::case(k, "final-step-zhuyin-n-28"),
        raw!(include!("fixtures/final_step_surface/b382bdf74b390e9d.rs")),
        None,
    );
}

// final-step-zhuyin-ni'h-20: MATCH.
#[test]
fn final_step_zhuyin_ni_apos_h_20() {
    run_j(
        "final_step_surface::final_step_zhuyin_ni_apos_h_20",
        Expect::RESULT,
        |k| b::case(k, "final-step-zhuyin-ni'h-20"),
        raw!(include!("fixtures/final_step_surface/fe69d01f415b1d0c.rs")),
        None,
    );
}

// final-step-zhuyin-ni'h-28: MATCH.
#[test]
fn final_step_zhuyin_ni_apos_h_28() {
    run_j(
        "final_step_surface::final_step_zhuyin_ni_apos_h_28",
        Expect::RESULT,
        |k| b::case(k, "final-step-zhuyin-ni'h-28"),
        raw!(include!("fixtures/final_step_surface/15c4d4a26d902250.rs")),
        None,
    );
}

// final-step-zhuyin-ni'hao-20: MATCH.
#[test]
fn final_step_zhuyin_ni_apos_hao_20() {
    run_j(
        "final_step_surface::final_step_zhuyin_ni_apos_hao_20",
        Expect::RESULT,
        |k| b::case(k, "final-step-zhuyin-ni'hao-20"),
        raw!(include!("fixtures/final_step_surface/529ea1064e800c9e.rs")),
        None,
    );
}

// final-step-zhuyin-ni-20: MATCH.
#[test]
fn final_step_zhuyin_ni_20() {
    run_j(
        "final_step_surface::final_step_zhuyin_ni_20",
        Expect::RESULT,
        |k| b::case(k, "final-step-zhuyin-ni-20"),
        raw!(include!("fixtures/final_step_surface/02063e6b8c7198a9.rs")),
        None,
    );
}

// final-step-zhuyin-nih-20: MATCH.
#[test]
fn final_step_zhuyin_nih_20() {
    run_j(
        "final_step_surface::final_step_zhuyin_nih_20",
        Expect::RESULT,
        |k| b::case(k, "final-step-zhuyin-nih-20"),
        raw!(include!("fixtures/final_step_surface/02063e6b8c7198a9.rs")),
        None,
    );
}

// final-step-zhuyin-nih-28: MATCH.
#[test]
fn final_step_zhuyin_nih_28() {
    run_j(
        "final_step_surface::final_step_zhuyin_nih_28",
        Expect::RESULT,
        |k| b::case(k, "final-step-zhuyin-nih-28"),
        raw!(include!("fixtures/final_step_surface/f1ba65a288101753.rs")),
        None,
    );
}

// final-step-zhuyin-nihao'-20: MATCH.
#[test]
fn final_step_zhuyin_nihao_apos_20() {
    run_j(
        "final_step_surface::final_step_zhuyin_nihao_apos_20",
        Expect::RESULT,
        |k| b::case(k, "final-step-zhuyin-nihao'-20"),
        raw!(include!("fixtures/final_step_surface/529ea1064e800c9e.rs")),
        None,
    );
}

// final-step-zhuyin-nihao-20: MATCH.
#[test]
fn final_step_zhuyin_nihao_20() {
    run_j(
        "final_step_surface::final_step_zhuyin_nihao_20",
        Expect::RESULT,
        |k| b::case(k, "final-step-zhuyin-nihao-20"),
        raw!(include!("fixtures/final_step_surface/6e8fcdc3d5ad56f7.rs")),
        None,
    );
}

// final-step-zhuyin-nihao-28: MATCH.
#[test]
fn final_step_zhuyin_nihao_28() {
    run_j(
        "final_step_surface::final_step_zhuyin_nihao_28",
        Expect::RESULT,
        |k| b::case(k, "final-step-zhuyin-nihao-28"),
        raw!(include!("fixtures/final_step_surface/6e8fcdc3d5ad56f7.rs")),
        None,
    );
}

// final-step-zhuyin-xi'an-20: MATCH.
#[test]
fn final_step_zhuyin_xi_apos_an_20() {
    run_j(
        "final_step_surface::final_step_zhuyin_xi_apos_an_20",
        Expect::RESULT,
        |k| b::case(k, "final-step-zhuyin-xi'an-20"),
        raw!(include!("fixtures/final_step_surface/0d9570381630a97f.rs")),
        None,
    );
}

// final-step-zhuyin-zhongguo-20: MATCH.
#[test]
fn final_step_zhuyin_zhongguo_20() {
    run_j(
        "final_step_surface::final_step_zhuyin_zhongguo_20",
        Expect::RESULT,
        |k| b::case(k, "final-step-zhuyin-zhongguo-20"),
        raw!(include!("fixtures/final_step_surface/9da3fc8d39610e94.rs")),
        None,
    );
}
