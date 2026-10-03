use chargeshare_core::{RateSchedule, RateWindow, UsdRate};

type RateResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Date {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

#[derive(Clone, Copy)]
struct DailyWindow {
    period: &'static str,
    start_hour: u8,
    end_hour: u8,
    usd_per_kwh: &'static str,
}

#[derive(Clone, Copy)]
pub(crate) struct RateVersion {
    id: &'static str,
    label: &'static str,
    effective_from: Date,
    effective_until: Date,
    windows: &'static [DailyWindow],
}

pub(crate) const SAMPLE_PUBLIC_TARIFF: &[RateVersion] = &[
    RateVersion {
        id: "ev2a-summer-2026",
        label: "Sample public EV2-A tariff",
        effective_from: Date {
            year: 2026,
            month: 9,
            day: 1,
        },
        effective_until: Date {
            year: 2026,
            month: 10,
            day: 1,
        },
        windows: &[
            DailyWindow {
                period: "Off-peak",
                start_hour: 0,
                end_hour: 15,
                usd_per_kwh: "0.2773931020",
            },
            DailyWindow {
                period: "Partial-peak",
                start_hour: 15,
                end_hour: 16,
                usd_per_kwh: "0.4783693788",
            },
            DailyWindow {
                period: "Peak",
                start_hour: 16,
                end_hour: 21,
                usd_per_kwh: "0.5866379696",
            },
            DailyWindow {
                period: "Partial-peak",
                start_hour: 21,
                end_hour: 24,
                usd_per_kwh: "0.4783693788",
            },
        ],
    },
    RateVersion {
        id: "ev2a-winter-2026-est",
        label: "Winter estimate, unverified",
        effective_from: Date {
            year: 2026,
            month: 10,
            day: 1,
        },
        effective_until: Date {
            year: 2027,
            month: 6,
            day: 1,
        },
        windows: &[
            DailyWindow {
                period: "Off-peak",
                start_hour: 0,
                end_hour: 15,
                usd_per_kwh: "0.2779120234",
            },
            DailyWindow {
                period: "Partial-peak",
                start_hour: 15,
                end_hour: 16,
                usd_per_kwh: "0.4465438926",
            },
            DailyWindow {
                period: "Peak",
                start_hour: 16,
                end_hour: 21,
                usd_per_kwh: "0.4624755018",
            },
            DailyWindow {
                period: "Partial-peak",
                start_hour: 21,
                end_hour: 24,
                usd_per_kwh: "0.4465438926",
            },
        ],
    },
];

pub(crate) fn version_label(id: &str) -> Option<&'static str> {
    SAMPLE_PUBLIC_TARIFF
        .iter()
        .find(|version| version.id == id)
        .map(|version| version.label)
}

pub(crate) fn period_label(id: &str, hour: i64) -> Option<&'static str> {
    SAMPLE_PUBLIC_TARIFF
        .iter()
        .find(|version| version.id == id)?
        .windows
        .iter()
        .find(|window| i64::from(window.start_hour) <= hour && hour < i64::from(window.end_hour))
        .map(|window| window.period)
}

pub(crate) fn schedule_for_days(
    versions: &[RateVersion],
    days: impl IntoIterator<Item = (Date, i64)>,
) -> RateResult<RateSchedule> {
    let mut windows = Vec::new();
    for (date, start_tick) in days {
        let mut matching = versions
            .iter()
            .filter(|version| version.effective_from <= date && date < version.effective_until);
        let Some(version) = matching.next() else {
            continue;
        };
        if matching.next().is_some() {
            return Err("overlapping rate versions".into());
        }
        for window in version.windows {
            if window.end_hour > 24 {
                return Err("rate window extends beyond its date".into());
            }
            windows.push(RateWindow::new(
                version.id,
                start_tick
                    .checked_add(i64::from(window.start_hour))
                    .ok_or("rate tick overflow")?,
                start_tick
                    .checked_add(i64::from(window.end_hour))
                    .ok_or("rate tick overflow")?,
                UsdRate::parse_per_kwh(window.usd_per_kwh)?,
            )?);
        }
    }
    Ok(RateSchedule::new(windows)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chargeshare_core::{
        ChargeType, ChargerClassification, ConnectionId, CounterReading, Energy, Event, EventKind,
        Ledger, OwnerId, PricingHold, SessionId, VehicleId,
    };

    fn date(month: u8, day: u8) -> Date {
        Date {
            year: 2026,
            month,
            day,
        }
    }

    #[test]
    fn date_boundaries_select_only_the_covering_version() {
        let september = SAMPLE_PUBLIC_TARIFF[0];
        let october = RateVersion {
            id: "sample-october",
            label: "Test rate",
            effective_from: date(10, 1),
            effective_until: date(11, 1),
            windows: &[DailyWindow {
                period: "Off-peak",
                start_hour: 0,
                end_hour: 24,
                usd_per_kwh: "0.50",
            }],
        };
        for (day, expected) in [
            (date(8, 31), None),
            (date(9, 1), Some(september.id)),
            (date(9, 30), Some(september.id)),
            (date(10, 1), Some(october.id)),
            (date(11, 1), None),
        ] {
            let rates = schedule_for_days(&[october, september], [(day, 100)]).unwrap();
            assert_eq!(rates.windows().first().map(RateWindow::version), expected);
            assert!(
                rates
                    .windows()
                    .iter()
                    .all(|window| Some(window.version()) == expected)
            );
        }
        assert!(schedule_for_days(&[september, september], [(date(9, 1), 0)]).is_err());
        let rates = schedule_for_days(&[september], [(date(9, 30), 0), (date(10, 1), 24)]).unwrap();
        assert_eq!(rates.windows().last().unwrap().end_time(), 24);
    }

    #[test]
    fn winter_boundaries_and_gap_never_reuse_a_neighbor() {
        for (date, expected) in [
            (date(9, 30), Some("ev2a-summer-2026")),
            (date(10, 1), Some("ev2a-winter-2026-est")),
            (
                Date {
                    year: 2027,
                    month: 5,
                    day: 31,
                },
                Some("ev2a-winter-2026-est"),
            ),
            (
                Date {
                    year: 2027,
                    month: 6,
                    day: 1,
                },
                None,
            ),
        ] {
            let rates = schedule_for_days(SAMPLE_PUBLIC_TARIFF, [(date, 0)]).unwrap();
            assert_eq!(rates.windows().first().map(RateWindow::version), expected);
        }
        let later = RateVersion {
            effective_from: date(10, 2),
            ..SAMPLE_PUBLIC_TARIFF[1]
        };
        let gap = schedule_for_days(&[SAMPLE_PUBLIC_TARIFF[0], later], [(date(10, 1), 0)]).unwrap();
        assert!(gap.windows().is_empty());
    }

    #[test]
    fn sample_windows_keep_all_decimal_places() {
        let rates = schedule_for_days(SAMPLE_PUBLIC_TARIFF, [(date(9, 13), 100)]).unwrap();
        let actual: Vec<_> = rates
            .windows()
            .iter()
            .map(|window| {
                (
                    window.start_time(),
                    window.end_time(),
                    window.rate().to_string(),
                )
            })
            .collect();
        assert_eq!(
            actual,
            vec![
                (100, 115, "0.2773931020 USD/kWh".to_owned()),
                (115, 116, "0.4783693788 USD/kWh".to_owned()),
                (116, 121, "0.5866379696 USD/kWh".to_owned()),
                (121, 124, "0.4783693788 USD/kWh".to_owned()),
            ]
        );
    }

    #[test]
    fn unchanged_evidence_can_be_repriced_or_held_without_rate_fallback() {
        let vehicle = VehicleId::new("sample-vehicle").unwrap();
        let connection = ConnectionId::new("sample-session").unwrap();
        let mut ledger = Ledger::default();
        ledger
            .register(vehicle.clone(), OwnerId::new("sample-owner").unwrap())
            .unwrap();
        for (position, time, kind, counter) in
            [(0, 1, EventKind::Start, "0"), (1, 3, EventKind::End, "10")]
        {
            ledger
                .ingest(Event {
                    vehicle: Some(vehicle.clone()),
                    connection: connection.clone(),
                    position,
                    time,
                    kind,
                    charge_type: ChargeType::Ac,
                    counter: Some(CounterReading::parse_kwh(counter)),
                })
                .unwrap();
        }
        ledger
            .classify(
                &vehicle,
                &SessionId {
                    vehicle: vehicle.clone(),
                    connection,
                },
                ChargerClassification::SharedCharger,
            )
            .unwrap();
        let original = schedule_for_days(SAMPLE_PUBLIC_TARIFF, [(date(9, 13), 0)]).unwrap();
        let first = ledger.pricing(&vehicle, &original).unwrap();
        assert_eq!(
            first.exact_priced_subtotal.to_string(),
            "2.7739310200000000 USD"
        );
        let replacement = RateVersion {
            id: "sample-revised",
            windows: &[DailyWindow {
                period: "Off-peak",
                start_hour: 0,
                end_hour: 24,
                usd_per_kwh: "0.30",
            }],
            ..SAMPLE_PUBLIC_TARIFF[0]
        };
        let changed = schedule_for_days(&[replacement], [(date(9, 13), 0)]).unwrap();
        let revised = ledger.pricing(&vehicle, &changed).unwrap();
        assert_eq!(
            revised.exact_priced_subtotal.to_string(),
            "3.0000000000000000 USD"
        );
        assert_eq!(first.sessions[0].session, revised.sessions[0].session);
        assert_eq!(first, ledger.pricing(&vehicle, &original).unwrap());
        for day in [
            date(8, 31),
            Date {
                year: 2027,
                month: 6,
                day: 1,
            },
        ] {
            let uncovered = schedule_for_days(SAMPLE_PUBLIC_TARIFF, [(day, 0)]).unwrap();
            let held = ledger.pricing(&vehicle, &uncovered).unwrap();
            assert!(matches!(
                held.sessions[0].quote,
                Err(PricingHold::MissingRate { .. })
            ));
            assert_eq!(held.priced_energy, Energy::ZERO);
            assert_eq!(held.sessions[0].session, first.sessions[0].session);
        }
    }
}
