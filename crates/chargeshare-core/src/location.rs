use std::fmt;

const EARTH_RADIUS_METERS: f64 = 6_371_008.8;

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub struct LocationPoint {
    latitude_bits: u64,
    longitude_bits: u64,
}

impl LocationPoint {
    pub fn new(latitude: f64, longitude: f64) -> Option<Self> {
        if !latitude.is_finite()
            || !longitude.is_finite()
            || !(-90.0..=90.0).contains(&latitude)
            || !(-180.0..=180.0).contains(&longitude)
        {
            return None;
        }
        Some(Self {
            latitude_bits: if latitude == 0.0 { 0.0 } else { latitude }.to_bits(),
            longitude_bits: if longitude == 0.0 { 0.0 } else { longitude }.to_bits(),
        })
    }

    pub fn latitude(self) -> f64 {
        f64::from_bits(self.latitude_bits)
    }

    pub fn longitude(self) -> f64 {
        f64::from_bits(self.longitude_bits)
    }
}

impl fmt::Debug for LocationPoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LocationPoint { coordinates: <redacted> }")
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum LocationSample {
    #[default]
    Missing,
    Invalid,
    Valid(LocationPoint),
    InsideGeofence,
    OutsideGeofence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChargerGeofence {
    center: LocationPoint,
    radius_meters: u32,
}

impl ChargerGeofence {
    pub fn new(center: LocationPoint, radius_meters: u32) -> Option<Self> {
        (radius_meters > 0).then_some(Self {
            center,
            radius_meters,
        })
    }

    pub fn contains(self, point: LocationPoint) -> bool {
        let latitude_delta = (point.latitude() - self.center.latitude()).to_radians();
        let longitude_delta = (point.longitude() - self.center.longitude()).to_radians();
        let haversine = (latitude_delta / 2.0).sin().powi(2)
            + self.center.latitude().to_radians().cos()
                * point.latitude().to_radians().cos()
                * (longitude_delta / 2.0).sin().powi(2);
        let distance = 2.0 * EARTH_RADIUS_METERS * haversine.clamp(0.0, 1.0).sqrt().asin();
        distance <= f64::from(self.radius_meters) + 1e-8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_positive_integer_radius_includes_its_boundary_and_excludes_points_beyond_it() {
        let center = LocationPoint::new(0.0, 0.0).unwrap();
        assert!(ChargerGeofence::new(center, 0).is_none());
        for radius in [1, 99, 100, 101, 250] {
            let geofence = ChargerGeofence::new(center, radius).unwrap();
            assert!(
                geofence.contains(
                    LocationPoint::new((f64::from(radius) / EARTH_RADIUS_METERS).to_degrees(), 0.0)
                        .unwrap()
                )
            );
            assert!(
                !geofence.contains(
                    LocationPoint::new(
                        ((f64::from(radius) + 0.001) / EARTH_RADIUS_METERS).to_degrees(),
                        0.0
                    )
                    .unwrap()
                )
            );
        }
    }
}
