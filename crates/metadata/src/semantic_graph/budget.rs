use std::{error::Error, fmt};
/// Independent hard ceilings; relationships are considered before bindings per layer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GraphBudget {
    depth: u8,
    assets: u8,
    relationships: u16,
    bindings: u16,
}
impl GraphBudget {
    /// Maximum traversed hops; the boundary is inspected without expanding it.
    pub const MAX_DEPTH: u8 = 4;
    /// Maximum unique assets, including the root.
    pub const MAX_ASSETS: u8 = 100;
    /// Maximum relationships or bindings of each kind, with a private look-ahead.
    pub const MAX_LINKS: u16 = 200;
    /// Validates budgets before allocating graph collections or opening storage.
    ///
    /// # Errors
    /// Rejects depth above four, assets outside 1–100, or either link kind outside 1–200.
    pub fn new(
        depth: u8,
        assets: u8,
        relationships: u16,
        bindings: u16,
    ) -> Result<Self, InvalidGraphBudget> {
        if depth > Self::MAX_DEPTH
            || !(1..=Self::MAX_ASSETS).contains(&assets)
            || !(1..=Self::MAX_LINKS).contains(&relationships)
            || !(1..=Self::MAX_LINKS).contains(&bindings)
        {
            return Err(InvalidGraphBudget);
        }
        Ok(Self {
            depth,
            assets,
            relationships,
            bindings,
        })
    }
    /// Returns maximum hops from the root.
    pub const fn depth(self) -> u8 {
        self.depth
    }
    /// Returns maximum unique assets.
    pub const fn assets(self) -> u8 {
        self.assets
    }
    /// Returns maximum included relationships.
    pub const fn relationships(self) -> u16 {
        self.relationships
    }
    /// Returns maximum included bindings.
    pub const fn bindings(self) -> u16 {
        self.bindings
    }
}
impl Default for GraphBudget {
    fn default() -> Self {
        Self {
            depth: 2,
            assets: 50,
            relationships: 100,
            bindings: 100,
        }
    }
}
/// Invalid graph request; never carries user-supplied identifiers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidGraphBudget;
impl fmt::Display for InvalidGraphBudget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid semantic graph budget")
    }
}
impl Error for InvalidGraphBudget {}
