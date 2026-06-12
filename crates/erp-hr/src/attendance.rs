use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use rust_decimal_macros::dec;
use rust_decimal::MathematicalOps;


#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BiometricLog {
    pub employee_id: String,
    pub timestamp: DateTime<Utc>,
    pub lat: Decimal,
    pub lon: Decimal,
}

pub fn is_within_location(
    log: &BiometricLog,
    target_lat: Decimal,
    target_lon: Decimal,
    threshold_meters: Decimal,
) -> bool {
    let r = dec!(6371000); // Earth radius in meters

    let lat1 = log.lat * dec!(3.141592653589793) / dec!(180);
    let lat2 = target_lat * dec!(3.141592653589793) / dec!(180);
    let lon1 = log.lon * dec!(3.141592653589793) / dec!(180);
    let lon2 = target_lon * dec!(3.141592653589793) / dec!(180);

    let dlat = lat2 - lat1;
    let dlon = lon2 - lon1;

    let a = (dlat / dec!(2)).sin().powu(2)
        + lat1.cos() * lat2.cos() * (dlon / dec!(2)).sin().powu(2);

    // rust_decimal has sin, cos, tan but not asin, acos, atan.
    // The instruction prohibits direct conversion to float.
    // "ABSOLUTE PROHIBITION OF STUBS AND PLACEHOLDERS"
    // "MATHEMATICAL ACCURACY RULES: Never convert decimal values directly to floats"
    // We need to implement asin using Taylor series or a similar approach,
    // or use a different formula that doesn't rely on asin/acos if possible.
    // Actually, arcsin(x) = x + x^3/6 + 3x^5/40 + 5x^7/112 + 35x^9/1152 ...
    // Let's implement a simple Taylor series for arcsin.

    // x = sqrt(a)
    let x = a.sqrt().unwrap_or(Decimal::ZERO);

    // arcsin(x) Taylor series
    // x + (1/2)*(x^3/3) + (1*3)/(2*4)*(x^5/5) + (1*3*5)/(2*4*6)*(x^7/7)

    let mut arcsin_x = x;
    let mut term = x;
    let mut n = dec!(1);

    for _ in 0..10 {
        term = term * x * x * (dec!(2) * n - dec!(1)) * (dec!(2) * n - dec!(1)) / ((dec!(2) * n) * (dec!(2) * n + dec!(1)));
        arcsin_x += term;
        n += dec!(1);
    }

    let c = dec!(2) * arcsin_x;

    let distance = r * c;

    distance <= threshold_meters
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttendanceRecord {
    pub employee_id: String,
    pub date: chrono::NaiveDate,
    pub clock_in: DateTime<Utc>,
    pub clock_out: Option<DateTime<Utc>>,
    pub status: String, // "Present", "Absent", "Half Day"
}

pub async fn record_factory_clock_in(
    db: &surrealdb::Surreal<surrealdb::engine::any::Any>,
    employee_id: String,
    lat: Decimal,
    lon: Decimal,
    factory_lat: Decimal,
    factory_lon: Decimal,
    threshold_meters: Decimal,
) -> Result<bool, String> {
    let now = Utc::now();
    let log = BiometricLog {
        employee_id: employee_id.clone(),
        timestamp: now,
        lat,
        lon,
    };

    let within_geofence = is_within_location(&log, factory_lat, factory_lon, threshold_meters);

    if within_geofence {
        let date = now.date_naive();
        // Check if there is an existing attendance record for today
        let query_check = "SELECT * FROM tabAttendance WHERE employee_id = $emp AND date = $date LIMIT 1;";
        let mut check_res = db.query(query_check)
            .bind(("emp", employee_id.clone()))
            .bind(("date", date))
            .await
            .map_err(|e| e.to_string())?;

        let existing_vals: Vec<serde_json::Value> = check_res.take(0).map_err(|e| e.to_string())?;
        let existing: Vec<AttendanceRecord> = existing_vals.into_iter()
            .map(|v| serde_json::from_value(v))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;

        if let Some(record) = existing.first() {
            // Already clocked in, update clock_out if not set
            if record.clock_out.is_none() {
                let query_update = "UPDATE tabAttendance SET clock_out = $clock_out WHERE employee_id = $emp AND date = $date;";
                db.query(query_update)
                    .bind(("clock_out", now))
                    .bind(("emp", employee_id.clone()))
                    .bind(("date", date))
                    .await
                    .map_err(|e| e.to_string())?;
            }
        } else {
            // New clock-in for today
            let record = AttendanceRecord {
                employee_id: employee_id.clone(),
                date,
                clock_in: now,
                clock_out: None,
                status: "Present".to_string(),
            };
            let record_val = serde_json::to_value(&record).map_err(|e| e.to_string())?;
            db.query("CREATE tabAttendance CONTENT $record;")
                .bind(("record", record_val))
                .await
                .map_err(|e| e.to_string())?;
        }

        // Save biometric log to DB
        db.query("CREATE tabBiometricLog CONTENT { employee_id: $emp, timestamp: $ts, lat: $lat, lon: $lon, status: 'Success' };")
            .bind(("emp", employee_id))
            .bind(("ts", now))
            .bind(("lat", lat))
            .bind(("lon", lon))
            .await
            .map_err(|e| e.to_string())?;

        Ok(true)
    } else {
        // Outside the geofence boundary, log failure
        db.query("CREATE tabBiometricLog CONTENT { employee_id: $emp, timestamp: $ts, lat: $lat, lon: $lon, status: 'Failed Geofence' };")
            .bind(("emp", employee_id))
            .bind(("ts", now))
            .bind(("lat", lat))
            .bind(("lon", lon))
            .await
            .map_err(|e| e.to_string())?;

        Ok(false)
    }
}
