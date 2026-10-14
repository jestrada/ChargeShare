import calendar
import json
import random
from datetime import datetime, timedelta, timezone
from pathlib import Path
from zoneinfo import ZoneInfo


SYNTHETIC_NOTICE = "Synthetic data for demos and screenshots only; not real"
OWNER = "owner-a"
VEHICLE = "vehicle-a"
FIXTURES = Path(__file__).resolve().parent
LOCAL_TIMEZONE = ZoneInfo("America/Los_Angeles")


def fictional_charger_location() -> dict[str, float]:
    complete = json.loads((FIXTURES / "complete.json").read_text())
    fields = complete["expected"][0]["payload"]["data"]
    return next(field["value"]["locationValue"] for field in fields if field["key"] == "Location")


def utc_timestamp(local_time: datetime) -> str:
    return local_time.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def counter_kwh(micro_kwh: int) -> str:
    return f"{micro_kwh // 1_000_000}.{micro_kwh % 1_000_000:06d}"


def session_schedule(day: int, random_source: random.Random) -> tuple[int, int]:
    if day % 9 == 0:
        return random_source.choice((980, 1080, 1185)), random_source.randrange(90, 181, 15)
    if day % 8 == 0:
        return random_source.choice((915, 1275, 1320)), random_source.randrange(75, 121, 15)
    return random_source.choice((0, 10, 20, 30, 45, 60)), random_source.randrange(135, 301, 15)


def generate_dashboard_fixture() -> dict:
    random_source = random.Random(70)
    manifest = json.loads((FIXTURES.parent / "ingestion-manifest.json").read_text())
    manifest["manifest_version"] = "fictional-dashboard-v1"
    manifest["mappings"] = [{"device": VEHICLE, "vehicle": VEHICLE, "owner": OWNER}]
    manifest["geofences"] = [geofence for geofence in manifest["geofences"] if geofence["owner"] == OWNER]
    manifest["owner_chargers"] = [charger for charger in manifest["owner_chargers"] if charger["owner_id"] == OWNER]
    manifest["annotations"] = []
    location = fictional_charger_location()
    records = []
    counter_micro_kwh = 100_000_000
    for month in range(6, 10):
        for day in range(1, calendar.monthrange(2026, month)[1] + 1):
            if day % 5 == 0:
                continue
            start_minute, duration_minutes = session_schedule(day, random_source)
            start = datetime(2026, month, day, tzinfo=LOCAL_TIMEZONE) + timedelta(minutes=start_minute)
            power_tenths_kw = random_source.randint(35, 72)
            energy_micro_kwh = power_tenths_kw * duration_minutes * 1_000_000 // 600
            connection = f"demo-{month:02d}-{day:02d}"
            for elapsed_minutes in range(0, duration_minutes + 1, 15):
                kind = "start" if elapsed_minutes == 0 else "end" if elapsed_minutes == duration_minutes else "sample"
                created_at = utc_timestamp(start + timedelta(minutes=elapsed_minutes))
                position = len(records) + 1
                observed_counter = counter_micro_kwh + energy_micro_kwh * elapsed_minutes // duration_minutes
                manifest["annotations"].append({
                    "device": VEHICLE,
                    "created_at": created_at,
                    "position": str(position),
                    "connection": connection,
                    "kind": kind,
                    "charge_type": "ac",
                })
                records.append({
                    "key": VEHICLE,
                    "payload": {
                        "vin": VEHICLE,
                        "createdAt": created_at,
                        "isResend": False,
                        "data": [
                            {"key": "DetailedChargeState", "value": {"detailedChargeStateValue": "DetailedChargeStateComplete" if kind == "end" else "DetailedChargeStateCharging"}},
                            {"key": "ACChargingEnergyIn", "value": {"stringValue": counter_kwh(observed_counter)}},
                            {"key": "Location", "value": {"locationValue": location}},
                        ],
                    },
                })
            counter_micro_kwh += energy_micro_kwh
    return {
        "notice": SYNTHETIC_NOTICE,
        "as_of_utc_seconds": int(datetime(2026, 9, 30, 23, 59, 59, tzinfo=LOCAL_TIMEZONE).timestamp()),
        "manifest": manifest,
        "records": records,
    }


if __name__ == "__main__":
    print(json.dumps(generate_dashboard_fixture(), indent=2))
