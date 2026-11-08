//! Final-step sentences and candidate windows of short full-pinyin inputs on both facades (#1175, #594). Generated from the frozen pin observations.

use crate::bodies::final_step_surface as b;
use crate::support::{Expect, run_j};

// final-step-pinyin-ba'ku-20: MATCH.
#[test]
fn final_step_pinyin_ba_apos_ku_20() {
    run_j(
        "final_step_surface::final_step_pinyin_ba_apos_ku_20",
        Expect::RESULT,
        |k| b::case(k, "final-step-pinyin-ba'ku-20"),
        raw!(include!("fixtures/final_step_surface/fb59a2cda2f9ad75.rs")),
        None,
    );
}

// final-step-pinyin-ba'kua-20: MATCH.
#[test]
fn final_step_pinyin_ba_apos_kua_20() {
    run_j(
        "final_step_surface::final_step_pinyin_ba_apos_kua_20",
        Expect::RESULT,
        |k| b::case(k, "final-step-pinyin-ba'kua-20"),
        raw!(include!("fixtures/final_step_surface/cd052169a3bae8a7.rs")),
        None,
    );
}

// final-step-pinyin-import-ba-kua: MATCH.
#[test]
fn final_step_pinyin_import_ba_kua() {
    run_j(
        "final_step_surface::final_step_pinyin_import_ba_kua",
        Expect::RESULT,
        |k| b::case(k, "final-step-pinyin-import-ba-kua"),
        raw!(include!("fixtures/final_step_surface/af5761d52e02d261.rs")),
        None,
    );
}

// final-step-pinyin-li'shi-20: MATCH.
#[test]
fn final_step_pinyin_li_apos_shi_20() {
    run_j(
        "final_step_surface::final_step_pinyin_li_apos_shi_20",
        Expect::RESULT,
        |k| b::case(k, "final-step-pinyin-li'shi-20"),
        raw!(include!("fixtures/final_step_surface/958e48bd76af0195.rs")),
        None,
    );
}

// final-step-pinyin-n-20: MATCH.
#[test]
fn final_step_pinyin_n_20() {
    run_j(
        "final_step_surface::final_step_pinyin_n_20",
        Expect::RESULT,
        |k| b::case(k, "final-step-pinyin-n-20"),
        raw!(include!("fixtures/final_step_surface/71bde32a4541b420.rs")),
        None,
    );
}

// final-step-pinyin-n-28: MATCH.
#[test]
fn final_step_pinyin_n_28() {
    run_j(
        "final_step_surface::final_step_pinyin_n_28",
        Expect::RESULT,
        |k| b::case(k, "final-step-pinyin-n-28"),
        raw!(include!("fixtures/final_step_surface/603aac058b5d396e.rs")),
        None,
    );
}

// final-step-pinyin-ni'h-20: MATCH.
#[test]
fn final_step_pinyin_ni_apos_h_20() {
    run_j(
        "final_step_surface::final_step_pinyin_ni_apos_h_20",
        Expect::RESULT,
        |k| b::case(k, "final-step-pinyin-ni'h-20"),
        raw!(include!("fixtures/final_step_surface/0eb1b1d47cb8fe17.rs")),
        None,
    );
}

// final-step-pinyin-ni'h-28: MATCH.
#[test]
fn final_step_pinyin_ni_apos_h_28() {
    run_j(
        "final_step_surface::final_step_pinyin_ni_apos_h_28",
        Expect::RESULT,
        |k| b::case(k, "final-step-pinyin-ni'h-28"),
        raw!(include!("fixtures/final_step_surface/aee51ce31dc73f90.rs")),
        None,
    );
}

// final-step-pinyin-ni'hao-20: MATCH.
#[test]
fn final_step_pinyin_ni_apos_hao_20() {
    run_j(
        "final_step_surface::final_step_pinyin_ni_apos_hao_20",
        Expect::RESULT,
        |k| b::case(k, "final-step-pinyin-ni'hao-20"),
        raw!(include!("fixtures/final_step_surface/7fe99d4e882ef3e6.rs")),
        None,
    );
}

// final-step-pinyin-ni-20: MATCH.
#[test]
fn final_step_pinyin_ni_20() {
    run_j(
        "final_step_surface::final_step_pinyin_ni_20",
        Expect::RESULT,
        |k| b::case(k, "final-step-pinyin-ni-20"),
        raw!(include!("fixtures/final_step_surface/ae288f094882e076.rs")),
        None,
    );
}

// final-step-pinyin-nih-20: MATCH.
#[test]
fn final_step_pinyin_nih_20() {
    run_j(
        "final_step_surface::final_step_pinyin_nih_20",
        Expect::RESULT,
        |k| b::case(k, "final-step-pinyin-nih-20"),
        raw!(include!("fixtures/final_step_surface/ae288f094882e076.rs")),
        None,
    );
}

// final-step-pinyin-nih-28: MATCH.
#[test]
fn final_step_pinyin_nih_28() {
    run_j(
        "final_step_surface::final_step_pinyin_nih_28",
        Expect::RESULT,
        |k| b::case(k, "final-step-pinyin-nih-28"),
        raw!(include!("fixtures/final_step_surface/a19d43d9ad8f223f.rs")),
        None,
    );
}

// final-step-pinyin-nihao'-20: MATCH.
#[test]
fn final_step_pinyin_nihao_apos_20() {
    run_j(
        "final_step_surface::final_step_pinyin_nihao_apos_20",
        Expect::RESULT,
        |k| b::case(k, "final-step-pinyin-nihao'-20"),
        raw!(include!("fixtures/final_step_surface/7fe99d4e882ef3e6.rs")),
        None,
    );
}

// final-step-pinyin-nihao-20: MATCH.
#[test]
fn final_step_pinyin_nihao_20() {
    run_j(
        "final_step_surface::final_step_pinyin_nihao_20",
        Expect::RESULT,
        |k| b::case(k, "final-step-pinyin-nihao-20"),
        raw!(include!("fixtures/final_step_surface/bdefe2ba9abde15d.rs")),
        None,
    );
}

// final-step-pinyin-nihao-28: MATCH.
#[test]
fn final_step_pinyin_nihao_28() {
    run_j(
        "final_step_surface::final_step_pinyin_nihao_28",
        Expect::RESULT,
        |k| b::case(k, "final-step-pinyin-nihao-28"),
        raw!(include!("fixtures/final_step_surface/bdefe2ba9abde15d.rs")),
        None,
    );
}

// final-step-pinyin-xi'an-20: MATCH.
#[test]
fn final_step_pinyin_xi_apos_an_20() {
    run_j(
        "final_step_surface::final_step_pinyin_xi_apos_an_20",
        Expect::RESULT,
        |k| b::case(k, "final-step-pinyin-xi'an-20"),
        raw!(include!("fixtures/final_step_surface/50b55005f4a5258a.rs")),
        None,
    );
}

// final-step-pinyin-zhongguo-20: MATCH.
#[test]
fn final_step_pinyin_zhongguo_20() {
    run_j(
        "final_step_surface::final_step_pinyin_zhongguo_20",
        Expect::RESULT,
        |k| b::case(k, "final-step-pinyin-zhongguo-20"),
        raw!(include!("fixtures/final_step_surface/16f02a119c319213.rs")),
        None,
    );
}
