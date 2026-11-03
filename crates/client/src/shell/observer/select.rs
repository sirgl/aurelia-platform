//! Which power the view of one power shows, and how Tab, the arrows and the digits move
//! between powers. Pure functions of the powers' state, so the same keys always lead to
//! the same power and the tests need no window.

/// Which way a key moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Way {
    Next,
    Previous,
}

/// The power after (or before) `from`, going round the powers and passing over the fallen.
/// When every other power has fallen the fallen ones are visited too, so a key always
/// moves while there is somewhere to move to.
pub fn step(from: usize, alive: &[bool], way: Way) -> usize {
    let count = alive.len();
    if count == 0 {
        return from;
    }
    let at = |offset: usize| match way {
        Way::Next => (from + offset) % count,
        Way::Previous => (from + count - offset % count) % count,
    };
    (1..count).map(at).find(|&power| alive[power]).or_else(|| (count > 1).then(|| at(1))).unwrap_or(from)
}

/// The power a digit key names (1 is the first), if the match has it.
pub fn by_digit(digit: u8, count: usize) -> Option<usize> {
    let index = usize::from(digit).checked_sub(1)?;
    (index < count).then_some(index)
}

/// The power to open on when a view is asked for without a choice: the one shown last, if
/// the match still has it, else the first that stands.
pub fn start(last: Option<usize>, alive: &[bool]) -> usize {
    last.filter(|&p| p < alive.len()).or_else(|| alive.iter().position(|&a| a)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_and_previous_go_round() {
        let alive = [true; 4];
        assert_eq!(step(0, &alive, Way::Next), 1);
        assert_eq!(step(3, &alive, Way::Next), 0);
        assert_eq!(step(0, &alive, Way::Previous), 3);
        assert_eq!(step(2, &alive, Way::Previous), 1);
    }

    #[test]
    fn the_fallen_are_passed_over() {
        let alive = [true, false, false, true, true];
        assert_eq!(step(0, &alive, Way::Next), 3);
        assert_eq!(step(4, &alive, Way::Next), 0);
        assert_eq!(step(3, &alive, Way::Previous), 0);
        assert_eq!(step(0, &alive, Way::Previous), 4);
    }

    #[test]
    fn when_nobody_else_stands_the_next_power_is_shown_all_the_same() {
        let alive = [false, true, false];
        assert_eq!(step(1, &alive, Way::Next), 2);
        assert_eq!(step(1, &alive, Way::Previous), 0);
        assert_eq!(step(0, &[false], Way::Next), 0);
        assert_eq!(step(0, &[], Way::Next), 0);
    }

    #[test]
    fn digits_name_powers_from_one() {
        assert_eq!(by_digit(1, 6), Some(0));
        assert_eq!(by_digit(6, 6), Some(5));
        assert_eq!(by_digit(7, 6), None);
        assert_eq!(by_digit(0, 6), None);
    }

    #[test]
    fn the_start_is_the_last_power_shown_or_the_first_that_stands() {
        assert_eq!(start(Some(2), &[true; 4]), 2);
        assert_eq!(start(Some(9), &[false, true]), 1);
        assert_eq!(start(None, &[false, false, true]), 2);
        assert_eq!(start(None, &[false]), 0);
    }
}
