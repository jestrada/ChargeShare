use std::collections::BTreeSet;

use crate::event::{ChargeType, CounterReading, EventKind};
use crate::identity::ConnectionId;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CounterLocation {
    Charger(String),
    Missing,
    Contradictory,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CounterObservation {
    pub time: i64,
    pub nanoseconds: u32,
    pub connection: Option<ConnectionId>,
    pub kind: Option<EventKind>,
    pub counter: Option<CounterReading>,
    pub charge_type: ChargeType,
    pub location: CounterLocation,
    pub breaks_connection: bool,
    pub starts_connection: bool,
    pub movement: bool,
}

impl CounterObservation {
    pub(crate) fn timestamp(&self) -> (i64, u32) {
        (self.time, self.nanoseconds)
    }
}

#[derive(Default)]
pub(crate) struct CounterWindow<'a> {
    pub(crate) before: Vec<&'a CounterObservation>,
    pub(crate) after: Vec<&'a CounterObservation>,
    pub(crate) uncertain_start: bool,
    pub(crate) uncertain_end: bool,
}

pub(crate) fn counter_window<'a>(
    connection: &ConnectionId,
    observations: &'a [CounterObservation],
) -> CounterWindow<'a> {
    let mut window = CounterWindow::default();
    let owned: Vec<_> = observations
        .iter()
        .filter(|observation| observation.connection.as_ref() == Some(connection))
        .collect();
    if !owned
        .iter()
        .any(|observation| observation.charge_type == ChargeType::Ac)
    {
        return window;
    }
    let chargers: BTreeSet<_> = owned
        .iter()
        .filter_map(|observation| match &observation.location {
            CounterLocation::Charger(charger) => Some(charger),
            _ => None,
        })
        .collect();
    let Some(charger) = chargers.first().filter(|_| chargers.len() == 1) else {
        return window;
    };
    for (kind, samples, uncertain) in [
        (
            EventKind::Start,
            &mut window.before,
            &mut window.uncertain_start,
        ),
        (EventKind::End, &mut window.after, &mut window.uncertain_end),
    ] {
        let Some(edge_index) = observations.iter().position(|observation| {
            observation.connection.as_ref() == Some(connection) && observation.kind == Some(kind)
        }) else {
            continue;
        };
        let edge = &observations[edge_index];
        if edge.movement
            || edge.breaks_connection
            || blocks_counter_continuity(edge, charger, observations)
        {
            continue;
        }
        let inside: Vec<_> = if kind == EventKind::Start {
            owned
                .iter()
                .copied()
                .filter(|observation| observation.timestamp() >= edge.timestamp())
                .collect()
        } else {
            owned
                .iter()
                .rev()
                .copied()
                .filter(|observation| observation.timestamp() <= edge.timestamp())
                .collect()
        };
        let mut previous = None;
        let mut blocked = false;
        for observation in inside {
            if blocks_counter_continuity(observation, charger, observations) {
                blocked = true;
                break;
            }
            if let Some(CounterReading::Valid(value)) = observation.counter {
                previous = Some(value);
                break;
            }
        }
        if blocked {
            continue;
        }
        let outside = if kind == EventKind::Start {
            &observations[..edge_index]
        } else {
            &observations[edge_index + 1..]
        };
        let mut outside = outside.iter();
        while let Some(observation) = if kind == EventKind::Start {
            outside.next_back()
        } else {
            outside.next()
        } {
            let distance = if kind == EventKind::Start {
                timestamp_nanoseconds(edge) - timestamp_nanoseconds(observation)
            } else {
                timestamp_nanoseconds(observation) - timestamp_nanoseconds(edge)
            };
            if !(1..=90_000_000_000).contains(&distance)
                || observation.connection.is_some()
                || observation.movement
                || blocks_counter_continuity(observation, charger, observations)
                || observation.breaks_connection
                || (kind == EventKind::End && observation.starts_connection)
            {
                break;
            }
            if has_competing_connection(observation, connection, kind, observations) {
                *uncertain = true;
                break;
            }
            if let Some(CounterReading::Valid(value)) = observation.counter {
                if previous.is_some_and(|previous| {
                    if kind == EventKind::Start {
                        value > previous
                    } else {
                        value < previous
                    }
                }) {
                    break;
                }
                samples.push(observation);
                previous = Some(value);
            }
            if observation.starts_connection {
                break;
            }
        }
        if kind == EventKind::Start {
            samples.reverse();
            if let Some(first) = samples.first()
                && let Some(prior) = observations.iter().rev().find(|observation| {
                    observation.timestamp() < first.timestamp()
                        && observation
                            .connection
                            .as_ref()
                            .is_some_and(|candidate| candidate != connection)
                        && observation.counter.is_some()
                })
                && matches!((first.counter, prior.counter), (Some(CounterReading::Valid(first)), Some(CounterReading::Valid(prior))) if first >= prior)
            {
                samples.clear();
                *uncertain = true;
            }
        }
    }
    window
}

fn has_competing_connection(
    observation: &CounterObservation,
    connection: &ConnectionId,
    kind: EventKind,
    observations: &[CounterObservation],
) -> bool {
    observations.iter().any(|other| {
        if other.connection.as_ref().is_none_or(|candidate| candidate == connection) {
            return false;
        }
        let distance = if kind == EventKind::Start && other.kind == Some(EventKind::End) {
            if matches!((observation.counter, other.counter), (Some(CounterReading::Valid(candidate)), Some(CounterReading::Valid(prior))) if candidate < prior) {
                return false;
            }
            timestamp_nanoseconds(observation) - timestamp_nanoseconds(other)
        } else if kind == EventKind::End && other.kind == Some(EventKind::Start) {
            timestamp_nanoseconds(other) - timestamp_nanoseconds(observation)
        } else {
            return false;
        };
        (0..=90_000_000_000).contains(&distance)
    })
}

fn blocks_counter_continuity(
    observation: &CounterObservation,
    charger: &str,
    observations: &[CounterObservation],
) -> bool {
    matches!(observation.counter, Some(CounterReading::Rejected(_)))
        || matches!(
            observation.charge_type,
            ChargeType::Dc | ChargeType::Ambiguous
        )
        || match &observation.location {
            CounterLocation::Charger(candidate) => candidate != charger,
            CounterLocation::Missing => false,
            CounterLocation::Contradictory => true,
        }
        || observations
            [observations.partition_point(|other| other.timestamp() < observation.timestamp())..]
            .iter()
            .take_while(|other| other.timestamp() == observation.timestamp())
            .any(|other| other != observation)
}

fn timestamp_nanoseconds(observation: &CounterObservation) -> i128 {
    i128::from(observation.time) * 1_000_000_000 + i128::from(observation.nanoseconds)
}
