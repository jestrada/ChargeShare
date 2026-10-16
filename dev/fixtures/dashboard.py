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


def generate_dashboard_fixture(multiple: bool = False) -> dict:
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
                            {"key": "BatteryLevel", "value": {"stringValue": "82" if kind == "end" else "45"}},
                        ],
                    },
                })
            counter_micro_kwh += energy_micro_kwh
    display_registry = [{"owner": OWNER, "vehicle": VEHICLE, "name": "Model 3 sample", "color": "#B8BDC3", "chargers": [{"id": manifest["owner_chargers"][0]["charger_id"], "name": "Driveway charger"}]}]
    if multiple:
        mapping = {"device": "vehicle-b", "vehicle": "vehicle-b", "owner": "owner-b"}
        manifest["mappings"].append(mapping)
        charger = json.loads(json.dumps(manifest["owner_chargers"][0]))
        charger["owner_id"] = "owner-b"
        charger["charger_id"] = "fictional-garage-charger"
        manifest["owner_chargers"].append(charger)
        original_records = list(records)
        original_annotations = list(manifest["annotations"])
        for annotation, record in zip(original_annotations, original_records, strict=True):
            annotation = dict(annotation, device="vehicle-b", connection=annotation["connection"].replace("demo-", "neighbor-"), position=str(len(records) + 1))
            record = json.loads(json.dumps(record))
            record["key"] = "vehicle-b"
            record["payload"]["vin"] = "vehicle-b"
            next(field for field in record["payload"]["data"] if field["key"] == "BatteryLevel")["value"]["stringValue"] = "67" if annotation["kind"] == "end" else "32"
            manifest["annotations"].append(annotation)
            records.append(record)
        display_registry.append({"owner": "owner-b", "vehicle": "vehicle-b", "name": "Model Y sample", "color": "#16181C", "chargers": [{"id": charger["charger_id"], "name": "Garage charger"}]})
    return {
        "notice": SYNTHETIC_NOTICE,
        "as_of_utc_seconds": int(datetime(2026, 9, 30, 23, 59, 59, tzinfo=LOCAL_TIMEZONE).timestamp()),
        "manifest": manifest,
        "display_registry": display_registry,
        "records": records,
    }


def serve_dashboard(directory):
    from functools import partial
    from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
    from urllib.parse import parse_qs, urlsplit

    class DashboardHandler(SimpleHTTPRequestHandler):
        def do_GET(self):
            target = urlsplit(self.path)
            selected = parse_qs(target.query).get("charger", [])
            if selected and target.path.endswith("/results"):
                import re
                if len(selected) != 1 or not re.fullmatch(r"[A-Za-z0-9_-]{1,64}", selected[0]):
                    self.send_error(400)
                    return
                self.path = target.path.removesuffix("/results") + "/chargers/" + selected[0] + "/results"
            super().do_GET()

    ThreadingHTTPServer(("127.0.0.1", 8788), partial(DashboardHandler, directory=str(directory))).serve_forever()


if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser(description=SYNTHETIC_NOTICE)
    parser.add_argument("--serve", type=Path)
    options = parser.parse_args()
    if options.serve:
        serve_dashboard(options.serve)
    else:
        print(json.dumps(generate_dashboard_fixture(multiple=True), indent=2))
