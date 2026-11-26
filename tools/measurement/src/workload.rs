//! The frozen keystroke corpus, checksum and profiler collection boundaries.
use crate::abi::{Guess, Handle, Typing};

pub(crate) const CYCLE_INPUTS: &[&str] = &[
    "ni",
    "wo",
    "de",
    "nihao",
    "zhongguo",
    "xian",
    "fangan",
    "xi'an",
    "bu'tian",
    "fan'gan",
    "n",
    "zh",
    "chongke",
    "caisho",
    "paolen",
    "waimenggu",
    "lenglan",
    "naoxion",
    "liangniejue",
    "chuaipengdengzaimiu",
];
pub(crate) const FNV_OFFSET: u64 = 1469598103934665603;
pub(crate) const FNV_PRIME: u64 = 1099511628211;

pub(crate) struct Cycle<'a> {
    pub(crate) typing: &'a Typing,
    pub(crate) guess: &'a Guess,
    pub(crate) instance: Handle,
    pub(crate) operations: u64,
    pub(crate) failed: bool,
}
impl Cycle<'_> {
    pub(crate) fn run(&mut self) -> u64 {
        let mut checksum = FNV_OFFSET;
        for input in CYCLE_INPUTS {
            if !(self.typing.reset)(self.instance) {
                self.failed = true;
                break;
            }
            let mut prefix = [0u8; 256];
            for (j, byte) in input.bytes().enumerate() {
                prefix[j] = byte;
                let parsed = (self.typing.parse)(self.instance, prefix.as_ptr().cast());
                let guessed = self.guess.call(self.instance);
                let mut count = 0;
                let counted = (self.typing.count)(self.instance, &mut count);
                self.operations += 1;
                if parsed == 0 || !guessed || !counted {
                    self.failed = true;
                }
                checksum ^= parsed as u64;
                checksum ^= if guessed {
                    0x9e3779b97f4a7c15
                } else {
                    0xbf58476d1ce4e5b9
                };
                checksum ^= if counted {
                    count as u64
                } else {
                    0x94d049bb133111eb
                };
                checksum = checksum.rotate_left(7);
            }
        }
        checksum
    }
    fn unit(&mut self, repeats: usize) -> u64 {
        let mut checksum = self.run();
        for _ in 1..repeats {
            checksum = (checksum << 1) ^ if checksum >> 63 != 0 { 0x1b } else { 0 };
            checksum ^= self.run();
        }
        checksum
    }
}

// These stable names are inspected by Python's nm checks. The collection
// requests keep the anchors distinct even with release LTO.
#[inline(never)]
#[unsafe(no_mangle)]
pub(crate) fn bisect_perf_cold_unit(cycle: &mut Cycle<'_>, repeats: usize) -> u64 {
    cycle.unit(std::hint::black_box(repeats))
}
#[inline(never)]
#[unsafe(no_mangle)]
pub(crate) fn bisect_perf_steady_unit(cycle: &mut Cycle<'_>, repeats: usize) -> u64 {
    let _collection = crate::platform::Collection::new();
    cycle.unit(repeats)
}

pub(crate) fn operations() -> u64 {
    CYCLE_INPUTS.iter().map(|s| s.len() as u64).sum()
}

pub(crate) fn count(name: &str, fallback: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|s| s.trim_start().parse().ok())
        .filter(|n| (1..=100000).contains(n))
        .unwrap_or(fallback)
}
