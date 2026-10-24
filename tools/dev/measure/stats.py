"""Shared startup/teardown statistics, moved from #1036 without changing its rules."""
import math
import random
import statistics

CONTROL_INTERVAL = (0.95, 1.05)
RESIDENCY_FLOOR_KIB = 64
RESIDENCY_FRACTION = 0.01


def percentile(values, fraction):
    """Linearly interpolate a quantile of sorted observations.

    Expect nonempty values and a fraction between zero and one; inputs are
    not validated and values are not mutated.
    """
    ordered = sorted(values)
    position = (len(ordered) - 1) * fraction
    lo, hi = math.floor(position), math.ceil(position)
    return ordered[lo] + (ordered[hi] - ordered[lo]) * (position - lo)


def summary(values):
    """Return distribution summaries for nonempty finite observations.

    Raise ValueError for empty or nonfinite input. range_over_median uses
    the absolute median and is None when that median is zero.
    """
    if not values or any(not math.isfinite(value) for value in values):
        raise ValueError('missing or nonfinite observations')
    middle = statistics.median(values)
    return dict(n=len(values), median=middle, minimum=min(values), maximum=max(values),
                p25=percentile(values, .25), p75=percentile(values, .75),
                range_over_median=(max(values) - min(values)) / abs(middle) if middle else None)


def paired_bootstrap(values):
    """Estimate the median and its 95% bootstrap percentile interval.

    Expect nonempty observations. Use 4000 same-size resamples and a local
    fixed-seed generator so repeated calls are deterministic.
    """
    generator = random.Random(0x074A2219)
    resamples = [statistics.median(generator.choices(values, k=len(values))) for _ in range(4000)]
    return dict(median=statistics.median(values), ci95=[percentile(resamples, .025), percentile(resamples, .975)])


def paired_ratio(numerator, denominator):
    """Bootstrap elementwise numerator/denominator ratios.

    Require equal nonempty samples and strictly positive denominators,
    otherwise raise ValueError. Pairing follows input order.
    """
    if len(numerator) != len(denominator) or not numerator or any(value <= 0 for value in denominator):
        raise ValueError('paired ratios require equal, positive-denominator samples')
    return paired_bootstrap([a / b for a, b in zip(numerator, denominator)])


def paired_difference(minuend, subtrahend):
    """Bootstrap elementwise minuend-minus-subtrahend differences.

    Require equal nonempty samples or raise ValueError; preserve input pairing.
    """
    if len(minuend) != len(subtrahend) or not minuend:
        raise ValueError('paired differences require equal, nonempty samples')
    return paired_bootstrap([a - b for a, b in zip(minuend, subtrahend)])


def within(estimate, lo, hi):
    """Test whether the entire ordered interval and its median fit inclusive bounds."""
    return lo <= estimate['ci95'][0] <= estimate['median'] <= estimate['ci95'][1] <= hi


def stable_control(ox, control):
    """Assess control/ox ratios against the inclusive 0.95 to 1.05 interval.

    Return the paired bootstrap estimate, stability flag, and applied bounds.
    """
    ratio = paired_ratio(control, ox)
    return dict(ratio=ratio, stable=within(ratio, *CONTROL_INTERVAL), rule=list(CONTROL_INTERVAL))


def stable_residency(ox, control):
    """Assess paired control-minus-ox residency differences in KiB.

    Require the median and full 95% interval to fit within plus or minus
    max(64 KiB, 1% of the absolute ox median). Return the estimate and rule.
    """
    # §7.5 item 2: d = control - ox in KiB; the median and the whole
    # interval lie within +-max(64 KiB, 1% of |median(ox)|), inclusive.
    difference = paired_difference(control, ox)
    band = max(RESIDENCY_FLOOR_KIB, RESIDENCY_FRACTION * abs(statistics.median(ox)))
    return dict(difference=difference, stable=within(difference, -band, band), bound_kib=band,
                rule=dict(floor_kib=RESIDENCY_FLOOR_KIB, fraction=RESIDENCY_FRACTION))


def exact_control(ox, control):
    """Compare deterministic control and ox counts with no tolerance.

    Return their values, control-minus-ox difference, and VALID or INVALID.
    """
    # §7.5 item 3: scoped instruction counts and deterministic heap
    # accounting must be equal. No tolerance: any difference is INVALID.
    return dict(rule='exact', ox=ox, control=control, difference=control - ox,
                verdict='VALID' if control == ox else 'INVALID')
