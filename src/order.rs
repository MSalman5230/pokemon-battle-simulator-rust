//! Ordering and speed ties shared by battle actions and event handlers.
//! Based on Pokémon Showdown's sim/battle.ts (MIT, see docs/LICENSE.pokemon-showdown).

use crate::rng::Rng;
use std::cmp::{Ordering, Reverse};

#[derive(Debug, Default, Copy, Clone, PartialEq)]
pub struct Priority {
    pub order: u32,
    /// Includes fractional priorities such as Quick Claw and Lagging Tail.
    pub priority: f64,
    pub speed: i32,
    pub sub_order: i32,
    pub effect_order: u32,
}

impl Priority {
    pub fn compare(self, other: Self) -> Ordering {
        const fn order(value: u32) -> u64 {
            if value == 0 { 1 << 32 } else { value as u64 }
        }
        fn priority(value: f64) -> f64 {
            if value.is_nan() { 0.0 } else { value }
        }
        order(self.order)
            .cmp(&order(other.order))
            .then_with(|| {
                priority(other.priority)
                    .partial_cmp(&priority(self.priority))
                    .unwrap()
            })
            .then_with(|| Reverse(self.speed).cmp(&Reverse(other.speed)))
            .then_with(|| self.sub_order.cmp(&other.sub_order))
            .then_with(|| self.effect_order.cmp(&other.effect_order))
    }
}

/// Select each winning group in upstream discovery order. Using a stable sort
/// and shuffling the resulting runs changes the inputs to later tie shuffles
/// when selection swaps displace their original elements.
pub fn speed_sort_by<T>(
    list: &mut [T],
    rng: &mut Rng,
    mut compare: impl FnMut(&T, &T) -> Ordering,
) {
    let mut sorted = 0;
    // All normal action queues fit inline. Event queues can be larger.
    let mut inline = [0; 16];
    let mut overflow = Vec::new();
    while sorted + 1 < list.len() {
        let mut best = sorted;
        let mut count = 1;
        inline[0] = sorted;
        overflow.clear();
        for i in sorted + 1..list.len() {
            match compare(&list[best], &list[i]) {
                Ordering::Less => {}
                Ordering::Greater => {
                    best = i;
                    inline[0] = i;
                    overflow.clear();
                    count = 1;
                }
                Ordering::Equal => {
                    if count < inline.len() {
                        inline[count] = i;
                    } else {
                        overflow.push(i);
                    }
                    count += 1;
                }
            }
        }
        for (offset, index) in inline[..count.min(inline.len())]
            .iter()
            .chain(&overflow)
            .copied()
            .enumerate()
        {
            list.swap(sorted + offset, index);
        }
        rng.shuffle(&mut list[sorted..sorted + count]);
        sorted += count;
    }
}
