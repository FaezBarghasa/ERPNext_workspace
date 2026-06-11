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
