use std::{collections::BTreeSet, io};

pub type CellCoord = (i32, i32);

pub fn active_grid(center: CellCoord) -> io::Result<BTreeSet<CellCoord>> {
    let mut cells = BTreeSet::new();
    let min_x = center
        .0
        .checked_sub(1)
        .ok_or_else(|| overflow_error(center))?;
    let max_x = center
        .0
        .checked_add(1)
        .ok_or_else(|| overflow_error(center))?;
    let min_y = center
        .1
        .checked_sub(1)
        .ok_or_else(|| overflow_error(center))?;
    let max_y = center
        .1
        .checked_add(1)
        .ok_or_else(|| overflow_error(center))?;

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            cells.insert((x, y));
        }
    }

    Ok(cells)
}

fn overflow_error(center: CellCoord) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("cell coordinate {center:?} cannot form a 3x3 active grid"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_grid_returns_three_by_three_neighbors() {
        let grid = active_grid((4, -2)).unwrap();

        assert_eq!(grid.len(), 9);
        assert!(grid.contains(&(3, -3)));
        assert!(grid.contains(&(4, -2)));
        assert!(grid.contains(&(5, -1)));
    }

    #[test]
    fn active_grid_rejects_overflowing_neighbors() {
        assert!(active_grid((i32::MIN, 0)).is_err());
        assert!(active_grid((0, i32::MAX)).is_err());
    }
}
