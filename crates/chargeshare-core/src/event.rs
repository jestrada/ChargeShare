use crate::energy::{CounterProblem, Energy};
use crate::identity::{ConnectionId, VehicleId};
use crate::location::LocationSample;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ChargeType {
    Ac,
    Dc,
    Ambiguous,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum EventKind {
    Start,
    Sample,
    Pause,
    Resume,
    End,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CounterReading {
    Valid(Energy),
    Rejected(CounterProblem),
}
impl CounterReading {
    pub fn parse_kwh(input: &str) -> Self {
        match Energy::parse_kwh(input) {
            Ok(value) => Self::Valid(value),
            Err(reason) => Self::Rejected(reason),
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Event {
    pub vehicle: Option<VehicleId>,
    pub connection: ConnectionId,
    pub position: u64,
    pub time: i64,
    pub kind: EventKind,
    pub charge_type: ChargeType,
    pub counter: Option<CounterReading>,
    pub location: LocationSample,
}
