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
/// ```
/// use binjs_shared::mru_delta::{ MRUDelta, Delta };
///
/// let mut mru = MRUDelta::new(3);
///
/// assert_eq!(mru.access(10),  Delta::TooFar,        "Introducing 10, within range from default 0 but small literal");
/// assert_eq!(mru.access(200), Delta::TooFar,        "Introducing 200, out of range from 10/0.");
/// assert_eq!(mru.access(200), Delta::Delta(0, 0),   "Just introduced 200");
/// assert_eq!(mru.access(200), Delta::Delta(0, 0),   "Just accessed 200");
/// assert_eq!(mru.access(201), Delta::Delta(0, 1),   "Introducing 201, one more than 200");
/// assert_eq!(mru.access(200), Delta::Delta(0, -1),  "Access 200 again");
/// assert_eq!(mru.access(202), Delta::Delta(0, 2),   "201 should not be in the MRU list, it should have been replaced by 200");
/// assert_eq!(mru.access(100), Delta::TooFar,        "100 is out of range -2^5");
/// assert_eq!(mru.access(131), Delta::Delta(0, 31),  "131 is just in range of 100");
/// assert_eq!(mru.access(99),  Delta::Delta(0, -32), "99 is *just* in range of 131");
/// assert_eq!(mru.access(202), Delta::Delta(1, 0),   "202 has been hanging around in MRU1");
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
        // Maximum negative value is reserved as a flag.
        let mut min_delta = (1i8 << (8 - self.size));
        // Find the entry with the smallest delta to value.
        for (index, entry) in self.items.iter().enumerate() {
            let delta = value as i64 - *entry as i64;
            if i64::abs(delta) < i64::abs(min_delta as i64) {
                min_index = index;
                min_delta = delta as i8;
            }
        }
        if min_delta < -32 || 31 < min_delta {
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
