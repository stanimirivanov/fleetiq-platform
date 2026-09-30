//! Internal mechanics shared by bounded keyset catalogue and audit pages.
//!
//! Public page sizes, pages, and cursors remain capability-specific. This
//! module centralizes only their common bound and entry/continuation storage.

use std::num::NonZeroU16;

const MAX_PAGE_SIZE: u16 = 100;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PageSize(NonZeroU16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PageSizeError {
    Zero,
    TooLarge { maximum: u16, actual: u16 },
}

impl PageSize {
    pub(crate) const fn new(value: u16) -> Result<Self, PageSizeError> {
        let Some(nonzero) = NonZeroU16::new(value) else {
            return Err(PageSizeError::Zero);
        };
        if value > MAX_PAGE_SIZE {
            Err(PageSizeError::TooLarge {
                maximum: MAX_PAGE_SIZE,
                actual: value,
            })
        } else {
            Ok(Self(nonzero))
        }
    }

    pub(crate) const fn get(self) -> u16 {
        self.0.get()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct KeysetPage<Entry, Cursor> {
    entries: Vec<Entry>,
    next: Option<Cursor>,
}

impl<Entry, Cursor> KeysetPage<Entry, Cursor> {
    pub(crate) fn new(entries: Vec<Entry>, next: Option<Cursor>) -> Self {
        Self { entries, next }
    }

    pub(crate) fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub(crate) const fn next(&self) -> Option<&Cursor> {
        self.next.as_ref()
    }
}

impl<Entry, Cursor: Copy> KeysetPage<Entry, Cursor> {
    pub(crate) const fn next_copied(&self) -> Option<Cursor> {
        self.next
    }
}
