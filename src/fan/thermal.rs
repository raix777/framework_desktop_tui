/// A temperature sensor's current reading.
///
/// Only sensors that report a valid temperature make it here; the ones the
/// EC reports as not present (or uncalibrated) are filtered out by the
/// controller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SensorReading {
    /// The sensor's name as reported by the EC (e.g. "APU", "Fan").
    name: String,
    celsius: u8,
}

impl SensorReading {
    pub fn new(name: String, celsius: u8) -> Self {
        Self { name, celsius }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn celsius(&self) -> u8 {
        self.celsius
    }
}

/// A fan's current speed reading.
///
/// Only fans that report a real speed make it here; an empty slot (no fan)
/// or a stalled fan is filtered out by the controller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FanReading {
    /// The fan's name as reported by the EC (e.g. "APU Fan", "Front Fan").
    name: String,
    rpm: u16,
}

impl FanReading {
    pub fn new(name: String, rpm: u16) -> Self {
        Self { name, rpm }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn rpm(&self) -> u16 {
        self.rpm
    }
}

/// A snapshot of the EC's thermal data: the present temperature sensors and
/// the fans, each with its current reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThermalSnapshot {
    sensors: Vec<SensorReading>,
    fans: Vec<FanReading>,
}

impl ThermalSnapshot {
    pub fn new(sensors: Vec<SensorReading>, fans: Vec<FanReading>) -> Self {
        Self { sensors, fans }
    }

    pub fn sensors(&self) -> &[SensorReading] {
        &self.sensors
    }

    pub fn fans(&self) -> &[FanReading] {
        &self.fans
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_keeps_sensor_and_fan_readings() {
        let snapshot = ThermalSnapshot::new(
            vec![SensorReading::new("APU".to_string(), 45)],
            vec![FanReading::new("APU Fan".to_string(), 2500)],
        );
        assert_eq!(snapshot.sensors().len(), 1);
        assert_eq!(snapshot.sensors()[0].name(), "APU");
        assert_eq!(snapshot.sensors()[0].celsius(), 45);
        assert_eq!(snapshot.fans().len(), 1);
        assert_eq!(snapshot.fans()[0].name(), "APU Fan");
        assert_eq!(snapshot.fans()[0].rpm(), 2500);
    }
}
