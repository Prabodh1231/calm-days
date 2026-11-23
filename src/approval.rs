//! The approval gate. One pure function decides whether a change must be approved first.
use chrono::{Duration, NaiveDateTime};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mv {
    pub id: i64,
    pub title: String,
    pub from: Option<NaiveDateTime>,
    pub to: NaiveDateTime,
    /// The target day already holds a fixed event.
    pub to_day_has_fixed_event: bool,
}

/// One card going to a list (and a place in it). Names are kept for the approval list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CardMove {
    pub id: i64,
    pub title: String,
    pub from_list: String,
    pub to_list: String,
    pub to_list_id: i64,
    /// Place in the target list; None = the end.
    pub index: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Change {
    /// Cards moved between lists or reordered (drag, "Move to...", "move all cards").
    MoveCards { moves: Vec<CardMove> },
    /// A list put away. Its cards leave the board with it.
    ArchiveList { id: i64, name: String, cards: usize },
    /// A manual move (drag or "Move to...").
    Move { moves: Vec<Mv> },
    /// Any planner / auto-reschedule output.
    Plan { moves: Vec<Mv> },
    Delete { ids: Vec<i64>, titles: Vec<String>, events: usize, tasks: usize, recurring_series: usize },
    /// Edit "all future" of a recurring series.
    EditFuture { id: i64, title: Option<String>, priority: Option<i64> },
}

/// True when the change must be shown as a list and approved before anything is written.
pub fn needs_approval(c: &Change) -> bool {
    match c {
        Change::MoveCards { moves } => moves.len() >= 3,
        Change::ArchiveList { cards, .. } => *cards >= 1,
        Change::Move { moves } => {
            moves.len() >= 3
                || moves.iter().any(|m| {
                    m.to_day_has_fixed_event
                        || m.from.map_or(false, |f| (m.to - f).abs() > Duration::days(7))
                })
        }
        // Planner only proposes, even for one item.
        Change::Plan { .. } => true,
        Change::Delete { events, tasks, recurring_series, .. } => *events >= 1 || *tasks >= 2 || *recurring_series >= 1,
        Change::EditFuture { .. } => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at(d: u32, h: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap().and_hms_opt(h, 0, 0).unwrap()
    }
    fn mv(id: i64, from: u32, to: u32, fixed: bool) -> Mv {
        Mv { id, title: "t".into(), from: Some(at(from, 10)), to: at(to, 15), to_day_has_fixed_event: fixed }
    }

    #[test]
    fn single_manual_move_is_free() {
        assert!(!needs_approval(&Change::Move { moves: vec![mv(1, 24, 24, false)] }));
        assert!(!needs_approval(&Change::Move { moves: vec![mv(1, 24, 25, false), mv(2, 24, 25, false)] }));
    }

    #[test]
    fn three_moves_need_approval() {
        let m = vec![mv(1, 24, 24, false), mv(2, 24, 24, false), mv(3, 24, 24, false)];
        assert!(needs_approval(&Change::Move { moves: m }));
    }

    #[test]
    fn long_move_and_fixed_event_day_need_approval() {
        assert!(needs_approval(&Change::Move { moves: vec![mv(1, 1, 20, false)] }));
        assert!(needs_approval(&Change::Move { moves: vec![mv(1, 24, 25, true)] }));
        // exactly 7 days and 5 hours is more than 7 days; exactly 7 days is not
        let exact = Mv { id: 1, title: "t".into(), from: Some(at(1, 10)), to: at(8, 10), to_day_has_fixed_event: false };
        assert!(!needs_approval(&Change::Move { moves: vec![exact] }));
    }

    #[test]
    fn planner_always_needs_approval_even_for_one_item() {
        assert!(needs_approval(&Change::Plan { moves: vec![mv(1, 24, 24, false)] }));
        assert!(needs_approval(&Change::Plan { moves: vec![] }));
    }

    #[test]
    fn delete_rules() {
        let del = |events, tasks, series| Change::Delete { ids: vec![], titles: vec![], events, tasks, recurring_series: series };
        assert!(!needs_approval(&del(0, 1, 0)));
        assert!(needs_approval(&del(1, 0, 0)));
        assert!(needs_approval(&del(0, 2, 0)));
        assert!(needs_approval(&del(0, 1, 1)));
    }

    #[test]
    fn card_moves_and_list_archive_rules() {
        let cm = |i| CardMove { id: i, title: "c".into(), from_list: "To do".into(), to_list: "Doing".into(), to_list_id: 2, index: None };
        assert!(!needs_approval(&Change::MoveCards { moves: vec![cm(1)] }));
        assert!(!needs_approval(&Change::MoveCards { moves: vec![cm(1), cm(2)] }));
        assert!(needs_approval(&Change::MoveCards { moves: vec![cm(1), cm(2), cm(3)] }));
        assert!(!needs_approval(&Change::ArchiveList { id: 1, name: "Empty".into(), cards: 0 }));
        assert!(needs_approval(&Change::ArchiveList { id: 1, name: "Full".into(), cards: 1 }));
    }

    #[test]
    fn edit_all_future_needs_approval() {
        assert!(needs_approval(&Change::EditFuture { id: 1, title: Some("x".into()), priority: None }));
    }
}
