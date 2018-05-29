//! A numbering scheme with offsets from recently used values.
// Deltas are in the h.o. 6 bits of a byte, range from -2^5 - 2^5-1
// When an item is not in range, the LRU value is evicted.
// When an item is in range of a delta, that value is evicted.
// Idea: initialize the buffer with 32, 96, 160, which would mean the initial MRU
// could reach 0-191
// Idea: Could bias the deltas to reach forward further than back on
// the assumption than things are often alphabetized.

use std::collections::LinkedList;
use std::iter;
use std::iter::FromIterator;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Delta {
    /// The entry is MRU item N plus a delta M.
    Delta(usize, i8),

    /// The entry can't be modeled as an MRU delta.
    TooFar,
}

/// A structure used to access values with repeated values in a small range.
///
/// TODO: below is out of date; literals which fit in 5 bits no longer go in the queue
/// ```
/// use binjs_shared::mru_delta::{ MRUDelta, Delta };
///
/// let mut mru = MRUDelta::new(3);
///
/// assert_eq!(mru.access(7), Delta::Delta(0, 7), "Introducing 7, offset from default 0");
/// assert_eq!(mru.access(7), Delta::Delta(0, 0),   "Just introduced 7");
/// assert_eq!(mru.access(7), Delta::Delta(0, 0),   "Just accessed 7);
/// assert_eq!(mru.access(8), Delta::Delta(0, 1), "Introducing 8, one more than 7");
/// assert_eq!(mru.access(7), Delta::Delta(0, -1), "Access 7 again");
/// assert_eq!(mru.access(9), Delta::Delta(0, 1), "Back to 8");
/// assert_eq!(mru.access(100), Delta::TooFar,   "100 is out of range 2^5-1");
/// assert_eq!(mru.access(131), Delta::Delta(0, 31), "131 is just in range of 100");
/// assert_eq!(mru.access(99), Delta::Delta(0, -32), "68 is just in range of 100");
/// assert_eq!(mru.access(0), Delta::Delta(1, -8),   "8 has been hanging around through delta updates");
///```
pub struct MRUDelta {
    // Maximum number of entries in the MRU
    pub size: usize,
    items: LinkedList<usize>,
}
impl MRUDelta {
    pub fn new(size: usize) -> Self {
        Self {
            size: size,
            // TODO: Consider pre-populating this with values spanning
            // -2^5 .. 2^5-1 so that frequent refs refer to this range
            items: LinkedList::from_iter(iter::repeat(0).take(size)),
        }
    }
    pub fn access(&mut self, value: usize) -> Delta {
        if value < 32 {
            // Small values can be encoded directly in a byte, so don't put them in the MRU list.
            return Delta::TooFar;
        }
        let mut min_index = self.size;
        // TODO: This prevents the maximum negative value.
        let mut min_delta = 1i8 << (8 - self.size);
        // Find the entry with the smallest delta to value.
        for (index, entry) in self.items.iter().enumerate() {
            let delta = i64::abs(*entry as i64 - value as i64);
            if delta < (min_delta as i64) {
                min_index = index;
                min_delta = delta as i8;
            }
        }
        if min_index == self.size {
            // Bit sad there's no swap_to_front?
            self.items.push_front(value);
            self.items.pop_back();
            return Delta::TooFar;
        } else if min_index == 0 {
            *self.items.front_mut().expect("MRU starts populated and maintains size") = value;
            return Delta::Delta(0, min_delta);
        } else {
            // Remove the interior item.
            let mut suffix = self.items.split_off(min_index);
            suffix.pop_front();
            self.items.append(&mut suffix);
            // Replace it at the start.
            self.items.push_front(value);
            return Delta::Delta(min_index, min_delta);
        }
    }
}
