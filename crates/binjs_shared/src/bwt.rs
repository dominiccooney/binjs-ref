use std::cmp::Ordering;
use std::fmt;
use std::fmt::Debug;
use std::ops::Index;
use std::vec::Vec;

use itertools::Itertools;

// A cycle of a string.
#[derive(Eq, PartialEq)]
struct BwtString<'a, C: 'a + Index<usize>> where C::Output : 'a {
    base: &'a C,
    size: usize,
    offset: usize,
}

// TODO: Is there a trait with 'len' which C can require? TrustedLen?
impl<'a, C: 'a + Index<usize>> BwtString<'a, C> {
    pub fn new(base: &'a C, size: usize, offset: usize) -> BwtString<'a, C> {
        Self {
            base,
            size,
            offset,
        }
    }

    pub fn last(&self) -> &C::Output {
        &self[self.size - 1]
    }
}

impl<'a, C: 'a + Index<usize>> PartialOrd for BwtString<'a, C> where Self : Ord {
    fn partial_cmp(&self, other: &BwtString<'a, C>) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[test]
fn bwt_string_ord() {
    let v = vec!['f', 'o', 'o', 'd'];
    assert_eq!(BwtString::new(&v, v.len(), 0).cmp(&BwtString::new(&v, v.len(), 0)),
               Ordering::Equal, "identity");
    assert_eq!(BwtString::new(&v, v.len(), 1).cmp(&BwtString::new(&v, v.len(), 0)),
               Ordering::Greater, "'o' after 'f'");

    let v = vec!['d', 'o', 'd', 'o'];
    assert_eq!(BwtString::new(&v, v.len(), 0).cmp(&BwtString::new(&v, v.len(), 2)),
               Ordering::Equal,
               "the rotations of this string are equal");
}

impl<'a, C: 'a> Ord for BwtString<'a, C> where C: Eq + Index<usize>, C::Output : Ord {
    fn cmp(&self, other: &BwtString<'a, C>) -> Ordering {
        if self.base == other.base &&
            self.offset == other.offset &&
            self.size == other.size {

            return Ordering::Equal;
        }
        let mut u = self.iter();
        let mut v = other.iter();
        loop {
            match (u.next(), v.next()) {
                (Some(u), Some(v)) => {
                    return match u.cmp(v) {
                        Ordering::Equal => continue,
                        otherwise => otherwise,
                    }
                },
                (None, None) => return Ordering::Equal,
                (None, _) => return Ordering::Less,
                (_, None) => return Ordering::Greater,
            }
        }
    }
}

impl<'a, C: 'a + Index<usize>> Index<usize> for BwtString<'a, C> {
    type Output = C::Output;

    fn index(&self, i: usize) -> &Self::Output {
        &self.base[(self.offset + i) % self.size]
    }
}

impl<'a, C: 'a + Index<usize>> BwtString<'a, C> {
    pub fn iter(&self) -> BwtStringIterator<C> {
        BwtStringIterator {
            s: &self,
            i: 0,
        }
    }
}

impl<'a, C: 'a + Eq + Index<usize>> Debug for BwtString<'a, C> where C::Output : Debug {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<")?;
        for c in self.iter() {
            write!(f, "{:?}", c)?;
        }
        write!(f, ">")
    }
}

#[test]
fn bwt_string_iterator() {
    let v = vec!['f', 'o', 'o', 'd'];

    let s = BwtString::new(&v, v.len(), 1);
    let mut i = s.iter();
    assert_eq!(i.next(), Some(&'o'));
    assert_eq!(i.next(), Some(&'o'));
    assert_eq!(i.next(), Some(&'d'));
    assert_eq!(i.next(), Some(&'f'));
    assert_eq!(i.next(), None);

    let s = BwtString::new(&v, 4, 0);
    let mut i = s.iter();
    assert_eq!(i.next(), Some(&'f'));
    assert_eq!(i.next(), Some(&'o'));
    assert_eq!(i.next(), Some(&'o'));
    assert_eq!(i.next(), Some(&'d'));
    assert_eq!(i.next(), None);
}

struct BwtStringIterator<'a, C: 'a + Index<usize>> {
    s: &'a BwtString<'a, C>,
    i: usize,
}

impl<'a, C: 'a + Index<usize>> Iterator for BwtStringIterator<'a, C> {
    type Item = &'a C::Output;
    fn next(&mut self) -> Option<Self::Item> {
        let i = self.i;
        self.i += 1;

        if i < self.s.size {
            Some(&self.s[i])
        } else {
            None
        }
    }
}

#[test]
fn bwt_test() {
    let v = vec!['b', 'a', 'n', 'a', 'n', 'a'];
    let (v_t, i) = bwt(&v);
    assert_eq!(vec!['n', 'n', 'b', 'a', 'a', 'a'], v_t);
    assert_eq!(3, i);
}

// Computes the Burrows-Wheeler transform of s and returns the
// transformed string and the index of the original string in
// the table.
pub fn bwt<T: Clone + Ord>(s: &Vec<T>) -> (Vec<T>, usize) {
    let mut transformed = Vec::with_capacity(s.len());
    let (i, found_end, end_i) = (0..s.len())
        .map(|i| BwtString::new(s, s.len(), i))
        .sorted()
        .into_iter()
        .map(|s| (s.offset, s.last().clone()))
        .fold((0, false, 0),
              |(i, found_end, end_i), (offset, ch)| {
                  if offset == 0 {
                      assert!(!found_end);
                  }
                  transformed.push(ch);
                  (i + 1,
                   found_end || offset == 0,
                   match offset {
                       0 => i,
                       _ => end_i,
                   })
              });
    assert!(found_end);
    assert_eq!(i, s.len());
    (transformed, end_i)
}

#[test]
fn bwt_ibwt_test() {
    let s = vec!['h', 'e', 'l', 'l', 'o'];
    let (t, i) = bwt(&s);
    let s_again = ibwt(&t, i);
    assert_eq!(s_again, s);
}

// This is specific to u8 because it uses a 256-element array of counts.
// Perhaps there's some trait for types with finite values, etc.
// This is cribbed from
// https://en.wikipedia.org/wiki/Burrows%E2%80%93Wheeler_transform#Sample_implementation
pub fn ibwt(s: &Vec<char>, end_marker: usize) -> Vec<char> {
    let first_col = s.iter().sorted();

    // The occurrence count of each letter.
    let mut count = Vec::with_capacity(1 << 8);
    count.resize(1 << 8, 0);

    // The index of the first occurrence of each letter.
    let mut byte_start = Vec::with_capacity(1 << 8);
    byte_start.resize(1 << 8, None);

    // Chain of redirections.
    let mut chain = Vec::with_capacity(1 << 8);
    chain.resize(1 << 8, None);

    for (i, ch) in s.iter().enumerate() {
        chain[i] = Some(count[*ch as usize]);
        count[*ch as usize] += 1;
        let prev_ch = *first_col[i];
        if byte_start[prev_ch as usize] == None {
            byte_start[prev_ch as usize] = Some(i);
        }
    }

    let mut result = Vec::with_capacity(s.len());
    result.resize(s.len(), '\0');
    let mut chain_i = end_marker;
    for i in 0..s.len() {
        let prev_ch = s[chain_i];
        result[s.len() - i - 1] = prev_ch as char;
        chain_i = byte_start[prev_ch as usize].expect("this char is used") + chain[chain_i].expect("this link is used");
    }

    result
}
