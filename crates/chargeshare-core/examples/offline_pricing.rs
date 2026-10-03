use chargeshare_core::{
    ChargeType, ChargerClassification, ConnectionId, CounterReading, Event, EventKind, Ledger,
    OwnerId, RateSchedule, RateWindow, SessionId, UsdRate, VehicleId,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut ledger = Ledger::default();
    let rates = RateSchedule::new(vec![
        RateWindow::new("fictional-low-v1", 0, 15, UsdRate::parse_per_kwh("0.20")?)?,
        RateWindow::new("fictional-high-v1", 15, 30, UsdRate::parse_per_kwh("0.40")?)?,
    ])?;
    let connection = ConnectionId::new("fictional-session")?;
    for (car, owner, samples) in [
        (
            "vehicle-a",
            "owner-a",
            vec![
                (1, 10, EventKind::Start, "0"),
                (2, 15, EventKind::Sample, "4"),
                (3, 20, EventKind::End, "10"),
            ],
        ),
        (
            "vehicle-b",
            "owner-b",
            vec![
                (1, 10, EventKind::Start, "0"),
                (3, 20, EventKind::End, "10"),
            ],
        ),
    ] {
        let vehicle = VehicleId::new(car)?;
        ledger.register(vehicle.clone(), OwnerId::new(owner)?)?;
        for (position, time, kind, counter) in samples {
            ledger.ingest(Event {
                vehicle: Some(vehicle.clone()),
                connection: connection.clone(),
                position,
                time,
                kind,
                charge_type: ChargeType::Ac,
                counter: Some(CounterReading::parse_kwh(counter)),
            })?;
        }
        ledger.classify(
            &vehicle,
            &SessionId {
                vehicle: vehicle.clone(),
                connection: connection.clone(),
            },
            ChargerClassification::SharedCharger,
        )?;
        let summary = ledger.pricing(&vehicle, &rates)?;
        println!("{}", summary.evidence_label);
        println!(
            "{}: priced {} for {}",
            car, summary.priced_energy, summary.rounded_priced_subtotal
        );
        for session in summary.sessions {
            match session.quote {
                Ok(quote) => {
                    for line in quote.lines {
                        println!(
                            "  ticks {}..{}: {} at {} ({}) = {}",
                            line.start_time,
                            line.end_time,
                            line.energy,
                            line.rate_window.rate(),
                            line.rate_window.version(),
                            line.exact_cost
                        );
                    }
                }
                Err(reason) => println!(
                    "  HELD: {reason:?}; observed {} is not included in the priced subtotal",
                    session.session.observed_ac
                ),
            }
        }
    }
    Ok(())
}
