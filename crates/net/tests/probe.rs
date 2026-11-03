//! A refusal must not tell a player what is on a tile it has not explored. Before the window
//! refused moves to unexplored tiles, `MoveUnit` into the dark answered "impassable" for water and
//! "accepted" or "no moves left" for land: a scout could chart the sea three tiles past its
//! sight for free. (Measured on seed 14: 20 water tiles all refused one way, 22 land tiles
//! another.) Now every unexplored destination gets the same answer.

mod common;

use common::*;
use kernel::command::KernelCommand;
use kernel::ids::{PlayerId, TileId};
use net::view::{SeatView, TileView};
use sandbox_rules::Command;

#[test]
fn a_move_into_the_dark_is_refused_the_same_way_over_water_and_land() {
    let rules = rules();
    let base = standard_game()(&rules, &spec(2, 2, 14));
    let view = SeatView::build(&base, &rules, PlayerId(0));
    let auth = base.view().auth;
    let scout = auth
        .units()
        .iter_handles()
        .find(|(_, c)| c.owner == PlayerId(0) && c.kind.0 == kind_named(&rules, "scout"))
        .unwrap();
    let dims = auth.map().dimensions;
    let mut reach: Vec<TileId> = vec![scout.1.tile];
    for _ in 0..4 {
        let mut next = reach.clone();
        for t in &reach {
            for n in dims.neighbors(*t) {
                if !next.contains(&n) {
                    next.push(n);
                }
            }
        }
        reach = next;
    }
    let mut answers = std::collections::BTreeSet::new();
    let (mut water, mut land) = (0, 0);
    for t in reach
        .into_iter()
        .filter(|t| matches!(view.tiles[t.0 as usize], TileView::Unknown))
    {
        let mut g = base.clone();
        g.open_window(1);
        let result = g.execute_as(
            PlayerId(0),
            &rules,
            Command::Kernel(KernelCommand::MoveUnit {
                unit: scout.0,
                destination: t,
            }),
        );
        answers.insert(format!("{result:?}").replace(|c: char| c.is_ascii_digit(), "#"));
        if auth.map().is_water(t) {
            water += 1
        } else {
            land += 1
        }
    }
    assert!(
        water > 5 && land > 5,
        "the probe covers both ({water} water, {land} land)"
    );
    assert_eq!(
        answers.len(),
        1,
        "one answer for every unexplored tile: {answers:?}"
    );
}
