//! The pin's three-parameter mixture math (`utils/training/k_mixture_model.h`
//! at `074a2219`), in `f64` as `parameter_t` is `double`.
//!
//! Counts are `guint32`; every subtraction here is the pin's unsigned
//! arithmetic and wraps, as `guint32` does.

/// `parameter_t`: the model math is `double` throughout.
pub type Parameter = f64;

/// `α = 1 − n_0 / N` (`k_mixture_model.h:40-43`).
#[must_use]
pub fn compute_alpha(n: u32, n_0: u32) -> Parameter {
    1.0 - Parameter::from(n_0) / Parameter::from(n)
}

/// `γ = 1 − n_1 / (N − n_0)` (`k_mixture_model.h:45-50`).
#[must_use]
pub fn compute_gamma(n: u32, n_0: u32, n_1: u32) -> Parameter {
    1.0 - Parameter::from(n_1) / Parameter::from(n.wrapping_sub(n_0))
}

/// `B = (T − n_1) / (N − n_0 − n_1)`, or `2` for the pin's special case
/// (`k_mixture_model.h:52-65`).
#[must_use]
pub fn compute_b(n: u32, t: u32, n_0: u32, n_1: u32) -> Parameter {
    let t_minus_n1 = t.wrapping_sub(n_1);
    let n_minus_n0_n1 = n.wrapping_sub(n_0).wrapping_sub(n_1);
    if t_minus_n1 == 0 && n_minus_n0_n1 == 0 {
        return 2.0;
    }
    Parameter::from(t_minus_n1) / Parameter::from(n_minus_n0_n1)
}

/// `Pr_G_3(k)` three-parameter mixture (`k_mixture_model.h:67-83`).
///
/// `k == 0 → 1 − α`; `k == 1 → α(1 − γ)`;
/// `k > 1 → (αγ/(B−1))·(1 − 1/(B−1))^{k−2}`. The pin's `abort()` for a
/// negative `k` is unreachable through the `u32` callers.
#[must_use]
pub fn compute_pr_g_3(k: u32, alpha: Parameter, gamma: Parameter, b: Parameter) -> Parameter {
    if k == 0 {
        return 1.0 - alpha;
    }
    if k == 1 {
        return alpha * (1.0 - gamma);
    }
    (alpha * gamma / (b - 1.0)) * (1.0 - 1.0 / (b - 1.0)).powf(Parameter::from(k - 2))
}

/// `compute_Pr_G_3_with_count` (`k_mixture_model.h:85-95`): derive
/// `(α, γ, B)` from the counts, then `Pr_G_3(k)`.
#[must_use]
pub fn compute_pr_g_3_with_count(k: u32, n: u32, t: u32, n_0: u32, n_1: u32) -> Parameter {
    let alpha = compute_alpha(n, n_0);
    let gamma = compute_gamma(n, n_0, n_1);
    let b = compute_b(n, t, n_0, n_1);
    compute_pr_g_3(k, alpha, gamma, b)
}
