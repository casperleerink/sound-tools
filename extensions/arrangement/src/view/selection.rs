//! What the composer has selected: a set of things with one of them first. Interface state,
//! never saved. Pure, no GPUI.
//!
//! The first one is what one thing at a time follows: the note editor shows the first selected
//! clip, and enter opens it. A click selects one thing; shift-click and cmd-click add a thing
//! or take it out again, as in the Finder. The timeline keeps its clips in one, and the note
//! editor can keep notes in another.

use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection<T: Ord> {
    items: BTreeSet<T>,
    /// Always one of `items`, or `None` when they are empty.
    primary: Option<T>,
}

impl<T: Ord> Default for Selection<T> {
    fn default() -> Self {
        Self {
            items: BTreeSet::new(),
            primary: None,
        }
    }
}

impl<T: Ord + Clone> Selection<T> {
    /// The one the rest follows, see the module.
    pub fn primary(&self) -> Option<&T> {
        self.primary.as_ref()
    }

    pub fn contains(&self, item: &T) -> bool {
        self.items.contains(item)
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items.iter()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Shift-click and cmd-click: a thing that is not selected is added and comes first, one
    /// that is selected goes out.
    pub fn toggle(&mut self, item: T) {
        if self.items.remove(&item) {
            if self.primary.as_ref() == Some(&item) {
                self.primary = self.items.first().cloned();
            }
        } else {
            self.items.insert(item.clone());
            self.primary = Some(item);
        }
    }

    /// Exactly these things, with `primary` first when it is one of them.
    pub fn set(&mut self, items: impl IntoIterator<Item = T>, primary: Option<T>) {
        self.items = items.into_iter().collect();
        self.primary = primary
            .filter(|primary| self.items.contains(primary))
            .or_else(|| self.items.first().cloned());
    }

    /// Keeps only the things for which `keep` holds, for example those the project still has.
    pub fn retain(&mut self, keep: impl FnMut(&T) -> bool) {
        self.items.retain(keep);
        if self
            .primary
            .as_ref()
            .is_some_and(|primary| !self.items.contains(primary))
        {
            self.primary = self.items.first().cloned();
        }
    }

    /// Takes a thing out, for example because it was deleted. Whether it was selected.
    pub fn remove(&mut self, item: &T) -> bool {
        let removed = self.items.remove(item);
        if self.primary.as_ref() == Some(item) {
            self.primary = self.items.first().cloned();
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_or_cmd_click_adds_and_takes_out() {
        let mut selection = Selection::default();
        selection.set([3], Some(3));
        assert_eq!((selection.len(), selection.primary()), (1, Some(&3)));
        selection.toggle(1);
        selection.toggle(2);
        assert_eq!(selection.iter().copied().collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(selection.primary(), Some(&2));
        // Out again: the first of the rest comes first.
        selection.toggle(2);
        assert_eq!(selection.primary(), Some(&1));
        assert!(!selection.contains(&2));
        selection.set([], None);
        assert!(selection.is_empty());
        assert_eq!(selection.primary(), None);
    }

    #[test]
    fn what_comes_first_is_always_selected() {
        let mut selection = Selection::default();
        selection.set([5, 4], None);
        assert_eq!(selection.primary(), Some(&4));
        assert!(selection.remove(&4));
        assert_eq!(selection.primary(), Some(&5));
        assert!(!selection.remove(&9));
        selection.set([7, 8], Some(9));
        assert_eq!(selection.primary(), Some(&7));
        selection.set([7, 8], Some(8));
        assert_eq!(selection.primary(), Some(&8));
        selection.retain(|item| *item != 8);
        assert_eq!(selection.primary(), Some(&7));
        assert_eq!(selection.len(), 1);
    }
}
