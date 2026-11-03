//! How many figures of a formation stand at a unit's health, and which ones.
//!
//! A unit is drawn as a group of figures (`UnitModel::slots`, one to six). With its health
//! at `hp` percent the group shows `ceil(hp / 100 * figures)` of them, at least one: a unit
//! at 100 % shows all, at 1 % the leader alone. Which figures go first is fixed per unit:
//! every follower gets a rank from a hash of the unit's persistent id and its slot, and the
//! leader (slot 0, the one the far marker draws) always ranks first. The same figures leave
//! as the health falls, and come back in the reverse order as it heals, so a unit's figures
//! never shuffle between frames.

/// Figures shown at `hp` percent (1..=100) of a formation of `total`.
pub fn visible_figures(total: usize, hp: u8) -> usize {
    if total == 0 {
        return 0;
    }
    let hp = usize::from(hp.clamp(1, 100));
    (hp * total).div_ceil(100).clamp(1, total)
}

fn hash(unit: u64, slot: usize) -> u64 {
    let mut x = unit.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (slot as u64 + 1).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 30;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 27)
}

/// Rank of figure `slot` of unit `unit` in a formation of `total`: 0 for the leader, then
/// 1.. in the order the followers stay. A figure stands while its rank is below the
/// visible count.
pub fn rank(unit: u64, slot: usize, total: usize) -> usize {
    if slot == 0 {
        return 0;
    }
    let mine = hash(unit, slot);
    1 + (1..total).filter(|other| *other != slot && (hash(unit, *other), *other) < (mine, slot)).count()
}

/// Does figure `slot` stand at `hp` percent?
pub fn stands(unit: u64, slot: usize, total: usize, hp: u8) -> bool {
    rank(unit, slot, total) < visible_figures(total, hp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_count_follows_health_and_never_drops_below_one() {
        assert_eq!(visible_figures(4, 100), 4);
        assert_eq!(visible_figures(4, 76), 4);
        assert_eq!(visible_figures(4, 75), 3);
        assert_eq!(visible_figures(4, 51), 3);
        assert_eq!(visible_figures(4, 50), 2);
        assert_eq!(visible_figures(4, 26), 2);
        assert_eq!(visible_figures(4, 25), 1);
        assert_eq!(visible_figures(4, 1), 1);
        assert_eq!(visible_figures(1, 1), 1);
        assert_eq!(visible_figures(6, 0), 1, "a corrupt zero still shows a figure");
        assert_eq!(visible_figures(3, 255), 3, "over 100 is whole");
        let mut last = 0;
        for hp in 1..=100u8 {
            let n = visible_figures(5, hp);
            assert!(n >= last, "monotone in health");
            last = n;
        }
    }

    #[test]
    fn ranks_are_a_permutation_with_the_leader_first_and_stable() {
        for unit in [1u64, 2, 77, 1 << 40] {
            for total in 1..=6 {
                let mut ranks: Vec<usize> = (0..total).map(|slot| rank(unit, slot, total)).collect();
                assert_eq!(rank(unit, 0, total), 0);
                ranks.sort_unstable();
                assert_eq!(ranks, (0..total).collect::<Vec<_>>(), "unit {unit} of {total}");
            }
        }
    }

    #[test]
    fn the_same_figures_leave_first_and_the_leader_stays() {
        let (unit, total) = (12345u64, 5);
        let mut standing: Vec<Vec<usize>> = Vec::new();
        for hp in (1..=100u8).rev() {
            standing.push((0..total).filter(|slot| stands(unit, *slot, total, hp)).collect());
        }
        for pair in standing.windows(2) {
            assert!(pair[1].iter().all(|slot| pair[0].contains(slot)), "a figure that left does not come back as health falls");
        }
        assert!(standing.iter().all(|s| s.contains(&0)), "the leader always stands");
        // Different units lose different figures first.
        let first_gone = |unit| (1..total).find(|slot| !stands(unit, *slot, total, 80)).unwrap();
        let firsts: std::collections::HashSet<_> = (1..40u64).map(first_gone).collect();
        assert!(firsts.len() > 1, "{firsts:?}");
    }
}
