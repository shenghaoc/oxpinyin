"""Shared startup/teardown statistics, moved from #1036 without changing its rules."""
import math
import random
import statistics

CONTROL_INTERVAL = (0.95, 1.05)
RESIDENCY_FLOOR_KIB = 64
RESIDENCY_FRACTION = 0.01


def percentile(values, fraction):
    ordered = sorted(values)
    position = (len(ordered) - 1) * fraction
    lo, hi = math.floor(position), math.ceil(position)
    return ordered[lo] + (ordered[hi] - ordered[lo]) * (position - lo)


def summary(values):
    if not values or any(not math.isfinite(value) for value in values):
        raise ValueError('missing or nonfinite observations')
    middle = statistics.median(values)
    return dict(n=len(values), median=middle, minimum=min(values), maximum=max(values),
                p25=percentile(values, .25), p75=percentile(values, .75),
                range_over_median=(max(values) - min(values)) / abs(middle) if middle else None)


def paired_bootstrap(values):
    generator = random.Random(0x074A2219)
    resamples = [statistics.median(generator.choices(values, k=len(values))) for _ in range(4000)]
    return dict(median=statistics.median(values), ci95=[percentile(resamples, .025), percentile(resamples, .975)])


def paired_ratio(numerator, denominator):
    if len(numerator) != len(denominator) or not numerator or any(value <= 0 for value in denominator):
        raise ValueError('paired ratios require equal, positive-denominator samples')
    return paired_bootstrap([a / b for a, b in zip(numerator, denominator)])


def paired_difference(minuend, subtrahend):
    if len(minuend) != len(subtrahend) or not minuend:
        raise ValueError('paired differences require equal, nonempty samples')
    return paired_bootstrap([a - b for a, b in zip(minuend, subtrahend)])


def within(estimate, lo, hi):
    return lo <= estimate['ci95'][0] <= estimate['median'] <= estimate['ci95'][1] <= hi


def stable_control(ox, control):
    ratio = paired_ratio(control, ox)
    return dict(ratio=ratio, stable=within(ratio, *CONTROL_INTERVAL), rule=list(CONTROL_INTERVAL))


def stable_residency(ox, control):
    # §7.5 item 2: d = control - ox in KiB; the median and the whole
    # interval lie within +-max(64 KiB, 1% of |median(ox)|), inclusive.
    difference = paired_difference(control, ox)
    band = max(RESIDENCY_FLOOR_KIB, RESIDENCY_FRACTION * abs(statistics.median(ox)))
    return dict(difference=difference, stable=within(difference, -band, band), bound_kib=band,
                rule=dict(floor_kib=RESIDENCY_FLOOR_KIB, fraction=RESIDENCY_FRACTION))


def exact_control(ox, control):
    # §7.5 item 3: scoped instruction counts and deterministic heap
    # accounting must be equal. No tolerance: any difference is INVALID.
    return dict(rule='exact', ox=ox, control=control, difference=control - ox,
                verdict='VALID' if control == ox else 'INVALID')
