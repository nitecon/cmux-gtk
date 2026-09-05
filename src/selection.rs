//! Selection policy shared by ordered workspace rows and sibling surface tabs.

/// Preserve the selected item after removal, or choose the item replacing its slot.
/// Falls back to the last remaining item; an empty collection has no selection.
/// Callers provide indices from the collection before removal and its remaining length.
pub(crate) fn after_removal(selected: usize, removed: usize, remaining: usize) -> Option<usize> {
    let last = remaining.checked_sub(1)?;
    Some(
        if removed < selected {
            selected - 1
        } else {
            selected
        }
        .min(last),
    )
}
