use std::collections::BTreeSet;

pub type CellCoord = (i32, i32);

#[must_use]
pub fn active_grid(center: CellCoord) -> BTreeSet<CellCoord> {
    let mut cells = BTreeSet::new();

    for y in center.1 - 1..=center.1 + 1 {
        for x in center.0 - 1..=center.0 + 1 {
            cells.insert((x, y));
        }
    }

    cells
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_grid_returns_three_by_three_neighbors() {
        let grid = active_grid((4, -2));

        assert_eq!(grid.len(), 9);
        assert!(grid.contains(&(3, -3)));
        assert!(grid.contains(&(4, -2)));
        assert!(grid.contains(&(5, -1)));
    }
}
