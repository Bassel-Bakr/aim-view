//! A list of at most N items, kept in an array with its length: a list with a small fixed limit needs no heap. The
//! report's small per-kill and per-run lists (matching.rs, measure.rs, summary.rs, tracking.rs, what_if.rs, the camera
//! and HUD watches) use it; it serializes as a JSON array.

use std::ops::{Deref, DerefMut};

use serde::{Serialize, Serializer};

/// At most N items, in the order pushed; the slots past the length hold `T::default()`. Pushing past N panics. It
/// derefs to a slice of the items, and serializes as one.
#[derive(Clone, Debug)]
pub struct Capped<T, const N: usize> {
    items: [T; N],
    len: usize,
}

impl<T: Default, const N: usize> Capped<T, N> {
    pub fn new() -> Capped<T, N> {
        Capped { items: std::array::from_fn(|_| T::default()), len: 0 }
    }

    pub fn push(&mut self, item: T) {
        assert!(self.len < N, "more than {N} items");
        self.items[self.len] = item;
        self.len += 1;
    }

    /// Takes out the item at `index`, moving the ones after it down.
    pub fn remove(&mut self, index: usize) -> T {
        self.items[index..self.len].rotate_left(1);
        self.len -= 1;
        std::mem::take(&mut self.items[self.len])
    }
}

impl<T: Default, const N: usize> Default for Capped<T, N> {
    fn default() -> Capped<T, N> {
        Capped::new()
    }
}

impl<T, const N: usize> Deref for Capped<T, N> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        &self.items[..self.len]
    }
}

impl<T, const N: usize> DerefMut for Capped<T, N> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.items[..self.len]
    }
}

impl<T: PartialEq, const N: usize> PartialEq for Capped<T, N> {
    fn eq(&self, other: &Capped<T, N>) -> bool {
        **self == **other
    }
}

impl<T: Default, const N: usize> FromIterator<T> for Capped<T, N> {
    fn from_iter<I: IntoIterator<Item = T>>(items: I) -> Capped<T, N> {
        let mut out = Capped::new();
        items.into_iter().for_each(|item| out.push(item));
        out
    }
}

impl<T, const N: usize> IntoIterator for Capped<T, N> {
    type Item = T;
    type IntoIter = std::iter::Take<std::array::IntoIter<T, N>>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter().take(self.len)
    }
}

impl<'a, T, const N: usize> IntoIterator for &'a Capped<T, N> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<T: Serialize, const N: usize> Serialize for Capped<T, N> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (**self).serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holds_up_to_its_limit_in_order() {
        let mut list: Capped<u32, 3> = [1, 2].into_iter().collect();
        list.push(3);
        assert_eq!(*list, [1, 2, 3]);
        assert_eq!(list.remove(0), 1);
        assert_eq!(*list, [2, 3]);
        assert_eq!(list.into_iter().collect::<Vec<_>>(), [2, 3]);
        assert_eq!(serde_json::to_string(&Capped::<u32, 4>::from_iter([5, 6])).unwrap(), "[5,6]");
    }

    #[test]
    #[should_panic(expected = "more than 2 items")]
    fn pushing_past_the_limit_panics() {
        let _: Capped<u32, 2> = [1, 2, 3].into_iter().collect();
    }
}
