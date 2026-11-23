//! Stage: history. An event-sourced simulation of `config.history.years`
//! years starting from the realms stage.
//!
//! History is stored as an append-only log of [`Event`]s plus periodic
//! [`HistoryState`] snapshots, never as mutable state. The state at any year
//! is the nearest earlier snapshot with the events after it replayed, and
//! always equals replaying every event from year 0. Later stages (trade,
//! diplomacy, disasters) add event kinds and simulation rules here.

mod disasters;
mod event;
mod sim;
mod state;
mod war;
mod wonders;

pub use event::{Cause, Event, EventKind, Flow, Grievance, PeaceTerms, Ruler};
pub use sim::{HistoryInputs, HistoryJob};
pub use state::{
    BattleRecord, Dragon, Eruption, HistoryState, LandmarkState, RealmState, Relation,
    SettlementState, WarState,
};

/// A finished history: the full event log and the snapshots that make
/// looking up any year fast.
#[derive(Clone, Debug, PartialEq)]
pub struct History {
    /// Years simulated after year 0.
    pub years: u32,
    /// Every event, ordered by `(year, seq)`.
    pub events: Vec<Event>,
    /// The state before any event.
    pub initial: HistoryState,
    snapshot_interval: u32,
    /// `snapshots[k]` is the state after every event up to year
    /// `k * snapshot_interval`.
    snapshots: Vec<HistoryState>,
}

impl History {
    /// The state at the end of `year` (clamped to the simulated range).
    pub fn state_at(&self, year: u32) -> HistoryState {
        let year = year.min(self.years);
        let k =
            ((year / self.snapshot_interval) as usize).min(self.snapshots.len().saturating_sub(1));
        let mut state = match self.snapshots.get(k) {
            Some(snapshot) => snapshot.clone(),
            None => self.initial.clone(),
        };
        self.advance_state(&mut state, year);
        state
    }

    /// Moves `state` forward to the end of `year` by applying the events in
    /// between. Cheap when stepping forward a little, as when playing.
    pub fn advance_state(&self, state: &mut HistoryState, year: u32) {
        let from = state.applied as usize;
        let to = self.events.partition_point(|e| e.year <= year);
        for event in &self.events[from..to.max(from)] {
            state.apply(event);
        }
        state.year = year.max(state.year);
    }

    /// Replays every event up to `year` onto the initial state, ignoring
    /// snapshots. The reference `state_at` must always agree with.
    pub fn replay_from_start(&self, year: u32) -> HistoryState {
        let mut state = self.initial.clone();
        for event in self.events.iter().take_while(|e| e.year <= year) {
            state.apply(event);
        }
        state.year = year.min(self.years);
        state
    }

    /// Index of the first event in `year` or later.
    pub fn first_event_at_or_after(&self, year: u32) -> usize {
        self.events.partition_point(|e| e.year < year)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WorldConfig;
    use crate::world::World;

    fn small(seed: u64) -> WorldConfig {
        let mut config = WorldConfig {
            seed,
            width: 96,
            height: 96,
            ..Default::default()
        };
        config.erosion.droplets = 3000;
        config.history.years = 400;
        config.history.snapshot_interval = 25;
        config
    }

    #[test]
    fn deterministic() {
        assert_eq!(
            World::generate(&small(3)).history,
            World::generate(&small(3)).history
        );
    }

    #[test]
    fn log_is_sorted_by_year_and_seq() {
        let history = World::generate(&small(42)).history;
        assert!(!history.events.is_empty());
        assert!(history
            .events
            .windows(2)
            .all(|p| (p[0].year, p[0].seq) < (p[1].year, p[1].seq)));
        assert!(history.events.iter().all(|e| e.year <= history.years));
    }

    #[test]
    fn snapshot_lookup_equals_full_replay() {
        let history = World::generate(&small(42)).history;
        for year in [0, 1, 24, 25, 26, 99, 137, 250, 311, 399, 400] {
            assert_eq!(
                history.state_at(year),
                history.replay_from_start(year),
                "year {year}"
            );
        }
    }

    #[test]
    fn stepping_forward_equals_lookup() {
        let history = World::generate(&small(7)).history;
        let mut state = history.state_at(10);
        for year in [11, 12, 40, 41, 160, 400] {
            history.advance_state(&mut state, year);
            assert_eq!(state, history.state_at(year), "year {year}");
        }
    }

    #[test]
    fn event_log_round_trips_through_json() {
        let history = World::generate(&small(42)).history;
        let json = serde_json::to_string(&history.events).unwrap();
        let back: Vec<Event> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, history.events);
    }

    #[test]
    fn event_rate_stays_sensible() {
        let mut config = small(42);
        config.history.years = 1000;
        let history = World::generate(&config).history;
        let per_century = history.events.len() as f32 / (history.years as f32 / 100.0);
        println!("events per century: {per_century:.0}");
        assert!(
            (20.0..4000.0).contains(&per_century),
            "{per_century} events per century"
        );
    }

    #[test]
    fn every_year_zero_realm_is_founded_with_a_ruler() {
        let world = World::generate(&small(42));
        let state = world.history.state_at(0);
        assert_eq!(state.realms.len(), world.realms.realms.len());
        assert!(state.realms.iter().all(|r| r.ruler.is_some()));
    }

    #[test]
    fn state_stays_consistent() {
        let history = World::generate(&small(42)).history;
        let state = history.state_at(history.years);
        for (id, realm) in state.realms.iter().enumerate() {
            let cells = state.owner.iter().filter(|&&o| o == id as u16).count() as u32;
            assert_eq!(realm.cells, cells, "realm {id} cell count");
            if !realm.alive {
                assert_eq!(cells, 0, "fallen realm {id} still owns land");
            }
        }
    }

    #[test]
    fn trade_conserves_goods() {
        let world = World::generate(&small(42));
        let mut seasons = 0;
        for e in &world.history.events {
            if let EventKind::TradeSeason { flows, tolls } = &e.kind {
                seasons += 1;
                for f in flows {
                    assert!(f.shipped > 0.0 && f.received > 0.0);
                    assert!(f.received <= f.shipped + 1e-4);
                    // At most half is lost, and nothing appears from nowhere.
                    assert!(f.received >= 0.5 * f.shipped - 1e-4);
                    let lost = f.shipped - f.received;
                    assert!((f.received + lost - f.shipped).abs() < 1e-4);
                    assert!(world.roads.distance(f.from, f.to).is_finite());
                    assert_ne!(f.from, f.to);
                }
                assert!(tolls.windows(2).all(|p| p[0].0 < p[1].0));
                assert!(tolls.iter().all(|&(r, t)| (r as usize)
                    < world.history.state_at(e.year).realms.len()
                    && t >= 0.0));
            }
        }
        assert!(seasons > 0);
    }

    #[test]
    fn trade_settings_change_only_history() {
        let base = World::generate(&small(42));
        let mut config = small(42);
        config.trade.toll_rate = 0.3;
        let changed = World::generate(&config);
        assert_eq!(base.roads, changed.roads);
        assert_eq!(base.resources, changed.resources);
        assert_ne!(base.history, changed.history);
    }

    #[test]
    fn history_settings_leave_earlier_stages_alone() {
        let base = World::generate(&small(42));
        let mut config = small(42);
        config.history.years = 200;
        config.politics.empire_formation = 0.9;
        let changed = World::generate(&config);
        assert_eq!(base.realms, changed.realms);
        assert_eq!(base.settlements, changed.settlements);
        assert_ne!(base.history, changed.history);
    }
}
