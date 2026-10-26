//! A directed tournament without an ordering promise.

use crate::EngineError;
use std::num::NonZeroU32;

/// In-place bracket: slot zero holds the overall winner; each other slot
/// holds the loser of the match whose right subtree starts at that slot.
/// IDs increase with leaf position. No candidate values are copied here.
#[derive(Clone, Debug, Default)]
pub(crate) struct LoserTree {
    matches: Vec<Option<NonZeroU32>>,
}

impl LoserTree {
    pub(crate) fn reset(
        &mut self,
        count: usize,
        indices: impl Iterator<Item = Option<usize>>,
        loses: &impl Fn(u32, u32) -> bool,
    ) -> Result<(), EngineError> {
        u32::try_from(count).map_err(|_| EngineError::TrellisTooLarge { candidates: count })?;
        let leaves = count
            .checked_next_power_of_two()
            .ok_or(EngineError::TrellisTooLarge { candidates: count })?;
        self.matches.clear();
        self.matches.resize(leaves, None);
        for (leaf, id) in indices.take(count).enumerate() {
            let Some(id) = id else {
                continue;
            };
            let id = id
                .checked_add(1)
                .and_then(|id| u32::try_from(id).ok())
                .and_then(NonZeroU32::new)
                .ok_or(EngineError::TrellisTooLarge { candidates: count })?;
            self.matches[leaf] = Some(id);
        }
        // This is the plain winner tree's reverse internal-node order:
        // right-to-left at each level, from leaf matches to the root.
        let mut width = 2;
        while width <= leaves {
            for left in (0..leaves).step_by(width).rev() {
                let right = left + width / 2;
                let (winner, loser) = Self::play(self.matches[left], self.matches[right], loses);
                self.matches[left] = winner;
                self.matches[right] = loser;
            }
            if width == leaves {
                break;
            }
            width *= 2;
        }
        Ok(())
    }

    fn play(
        a: Option<NonZeroU32>,
        b: Option<NonZeroU32>,
        loses: &impl Fn(u32, u32) -> bool,
    ) -> (Option<NonZeroU32>, Option<NonZeroU32>) {
        match (a, b) {
            (Some(a), Some(b)) => {
                let (left, right) = if a < b { (a, b) } else { (b, a) };
                if loses(left.get() - 1, right.get() - 1) {
                    (Some(right), Some(left))
                } else {
                    (Some(left), Some(right))
                }
            }
            (a, b) => (a.or(b), None),
        }
    }

    pub(crate) fn pop(
        &mut self,
        replay: bool,
        leaf_of: impl Fn(u32) -> usize,
        loses: &impl Fn(u32, u32) -> bool,
    ) -> Option<u32> {
        let winner = self.matches.first().copied().flatten()?.get() - 1;
        self.matches[0] = None;
        if replay {
            let leaf = leaf_of(winner);
            let mut challenger = None;
            let mut width = 2;
            while width <= self.matches.len() {
                let opponent = leaf / width * width + width / 2;
                let (next, loser) = Self::play(challenger, self.matches[opponent], loses);
                challenger = next;
                self.matches[opponent] = loser;
                if width == self.matches.len() {
                    break;
                }
                width *= 2;
            }
            self.matches[0] = challenger;
        }
        Some(winner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A tournament over fixed leaf positions. A match always calls `loses`
    /// with the lower leaf index on the left; false keeps that left leaf.
    /// There is no `Ord` promise: contradictory pairs and ties are permitted.
    #[derive(Clone, Debug, Default)]
    pub(crate) struct WinnerTree {
        winners: Vec<usize>,
        leaves: usize,
    }

    impl WinnerTree {
        pub(crate) fn reset(
            &mut self,
            count: usize,
            indices: impl Iterator<Item = Option<usize>>,
            loses: &impl Fn(usize, usize) -> bool,
        ) {
            self.leaves = count.next_power_of_two();
            self.winners.clear();
            self.winners.resize(self.leaves * 2, usize::MAX);
            for (index, storage) in indices.take(count).enumerate() {
                self.winners[self.leaves + index] = storage.unwrap_or(usize::MAX);
            }
            for index in (1..self.leaves).rev() {
                self.update(index, loses);
            }
        }

        fn update(&mut self, index: usize, loses: &impl Fn(usize, usize) -> bool) {
            self.winners[index] = match (self.winners[index * 2], self.winners[index * 2 + 1]) {
                (usize::MAX, right) => right,
                (left, usize::MAX) => left,
                (left, right) => {
                    if loses(left, right) {
                        right
                    } else {
                        left
                    }
                }
            };
        }

        pub(crate) fn pop(&mut self, loses: &impl Fn(usize, usize) -> bool) -> Option<usize> {
            let winner = self
                .winners
                .get(1)
                .copied()
                .filter(|&index| index != usize::MAX)?;
            // Follow the winner through the tree. Storage indices need not be
            // compact: node slots are packed into leaves in creation order.
            let mut index = 1;
            while index < self.leaves {
                index = if self.winners[index * 2] == winner {
                    index * 2
                } else {
                    index * 2 + 1
                };
            }
            self.winners[index] = usize::MAX;
            while index > 1 {
                index /= 2;
                self.update(index, loses);
            }
            Some(winner)
        }
    }

    fn compare(present: &[bool], stride: usize, relation: impl Fn(usize, usize) -> bool) {
        let actual_trace = RefCell::new(Vec::new());
        let reference_trace = RefCell::new(Vec::new());
        let actual_less = |left: u32, right: u32| {
            assert!(left < right);
            actual_trace
                .borrow_mut()
                .push((left as usize, right as usize));
            relation(left as usize / stride, right as usize / stride)
        };
        let reference_less = |left: usize, right: usize| {
            reference_trace.borrow_mut().push((left, right));
            relation(left / stride, right / stride)
        };
        let mut actual = LoserTree::default();
        let mut reference = WinnerTree::default();
        actual
            .reset(
                present.len(),
                (0..present.len()).map(|i| present[i].then_some(i * stride)),
                &actual_less,
            )
            .unwrap();
        reference.reset(
            present.len(),
            (0..present.len()).map(|i| present[i].then_some(i * stride)),
            &reference_less,
        );
        let count = present.iter().filter(|&&p| p).count();
        for i in 0..count {
            assert_eq!(
                actual
                    .pop(i + 1 < count, |id| id as usize / stride, &actual_less)
                    .map(|id| id as usize),
                reference.pop(&reference_less)
            );
            assert_eq!(*actual_trace.borrow(), *reference_trace.borrow());
        }
        assert_eq!(
            actual.pop(false, |id| id as usize / stride, &actual_less),
            None
        );
        assert_eq!(reference.pop(&reference_less), None);
        assert_eq!(*actual_trace.borrow(), *reference_trace.borrow());
    }

    #[test]
    fn every_four_leaf_direction_and_occupancy_matches_the_winner_reference() {
        for mask in 0..4096u16 {
            let relation = |left: usize, right: usize| {
                // Twelve independent directed edges, including contradictions.
                let bit = left * 3 + right - usize::from(right > left);
                mask & (1 << bit) != 0
            };
            for occupancy in 0..16 {
                compare(
                    &(0..4)
                        .map(|i| occupancy & (1 << i) != 0)
                        .collect::<Vec<_>>(),
                    1,
                    relation,
                );
            }
        }
    }

    #[test]
    fn seeded_sparse_trees_up_to_128_leaves_match_winners_and_call_traces() {
        let mut seed = 0x1297u64;
        for count in 0..=128 {
            for _ in 0..8 {
                let present: Vec<_> = (0..count)
                    .map(|_| {
                        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                        seed >> 62 != 0
                    })
                    .collect();
                let salt = seed;
                compare(&present, 1 + (salt as usize & 1), |left, right| {
                    let word = salt
                        ^ (left as u64).wrapping_mul(0x9e3779b97f4a7c15)
                        ^ (right as u64).wrapping_mul(0x517cc1b727220a95);
                    word.count_ones() & 1 != 0
                });
            }
        }
    }

    #[test]
    fn the_last_requested_extraction_makes_no_more_comparisons() {
        let calls = RefCell::new(Vec::new());
        let loses = |left, right| {
            calls.borrow_mut().push((left, right));
            false
        };
        let mut tree = LoserTree::default();
        tree.reset(4, (0..4).map(Some), &loses).unwrap();
        let before = calls.borrow().clone();
        assert_eq!(tree.pop(false, |id| id as usize, &loses), Some(0));
        assert_eq!(*calls.borrow(), before);
    }

    #[test]
    fn oversized_candidate_count_is_an_error_before_allocation() {
        if let Ok(count) = usize::try_from(u64::from(u32::MAX) + 1) {
            assert!(
                LoserTree::default()
                    .reset(count, std::iter::empty(), &|_, _| false)
                    .is_err()
            );
        }
        assert!(
            LoserTree::default()
                .reset(1, [Some(u32::MAX as usize)].into_iter(), &|_, _| false)
                .is_err()
        );
    }
}
