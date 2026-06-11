use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use rust_decimal_macros::dec;
use rust_decimal::MathematicalOps;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum InterestMethod {
    EMI,             // Equal Monthly Installments (fixed periodic payment)
    ReducingBalance, // Reducing Balance (fixed principal payments, reducing total payments)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AmortizationRow {
    pub period: u32,
    pub opening_balance: Decimal,
    pub principal_payment: Decimal,
    pub interest_payment: Decimal,
    pub total_payment: Decimal,
    pub closing_balance: Decimal,
}

/// Calculate a loan amortization schedule using either the EMI or Reducing Balance interest method.
pub fn calculate_amortization(
    principal: Decimal,
    annual_rate: Decimal,
    periods: u32,
    method: InterestMethod,
) -> Vec<AmortizationRow> {
    if periods == 0 {
        return vec![];
    }

    let r = annual_rate / dec!(12);
    let mut schedule = Vec::new();
    let mut current_balance = principal;

    match method {
        InterestMethod::EMI => {
            // EMI formula: M = P * [ r * (1+r)^n ] / [ (1+r)^n - 1 ]
            let one_plus_r = dec!(1) + r;
            let one_plus_r_pow_n = one_plus_r.powu(periods as u64);

            let m = if r.is_zero() {
                principal / Decimal::from(periods)
            } else {
                (principal * (r * one_plus_r_pow_n) / (one_plus_r_pow_n - dec!(1))).round_dp(2)
            };

            for i in 1..=periods {
                let interest = (current_balance * r).round_dp(2);
                let mut principal_payment = m - interest;

                // Adjust for last period to avoid rounding drift
                if i == periods {
                    principal_payment = current_balance;
                }

                let total_payment = principal_payment + interest;
                let closing_balance = current_balance - principal_payment;

                schedule.push(AmortizationRow {
                    period: i,
                    opening_balance: current_balance.round_dp(2),
                    principal_payment: principal_payment.round_dp(2),
                    interest_payment: interest,
                    total_payment: total_payment.round_dp(2),
                    closing_balance: closing_balance.round_dp(2),
                });

                current_balance = closing_balance;
            }
        }
        InterestMethod::ReducingBalance => {
            // Reducing Balance: principal payment is fixed every period (P / n)
            let fixed_principal = (principal / Decimal::from(periods)).round_dp(2);

            for i in 1..=periods {
                let interest = (current_balance * r).round_dp(2);
                let mut principal_payment = fixed_principal;

                // Adjust for last period to clear remaining balance exactly
                if i == periods {
                    principal_payment = current_balance;
                }

                let total_payment = principal_payment + interest;
                let closing_balance = current_balance - principal_payment;

                schedule.push(AmortizationRow {
                    period: i,
                    opening_balance: current_balance.round_dp(2),
                    principal_payment: principal_payment.round_dp(2),
                    interest_payment: interest,
                    total_payment: total_payment.round_dp(2),
                    closing_balance: closing_balance.round_dp(2),
                });

                current_balance = closing_balance;
            }
        }
    }

    schedule
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_emi_amortization() {
        let principal = dec!(10000);
        let annual_rate = dec!(0.12); // 12% annual -> 1% monthly
        let periods = 12;

        let schedule = calculate_amortization(principal, annual_rate, periods, InterestMethod::EMI);

        assert_eq!(schedule.len(), 12);
        // Verify final closing balance is exactly 0
        assert_eq!(schedule[11].closing_balance, Decimal::ZERO);

        // Verify total payment is constant (subject to final period rounding)
        let first_payment = schedule[0].total_payment;
        for row in &schedule[..11] {
            assert_eq!(row.total_payment, first_payment);
        }
    }

    #[test]
    fn test_reducing_balance_amortization() {
        let principal = dec!(12000);
        let annual_rate = dec!(0.12); // 12% annual -> 1% monthly
        let periods = 12;

        let schedule = calculate_amortization(principal, annual_rate, periods, InterestMethod::ReducingBalance);

        assert_eq!(schedule.len(), 12);
        // Fixed principal payment = 12000 / 12 = 1000
        for row in &schedule {
            assert_eq!(row.principal_payment, dec!(1000));
        }

        // Verify final closing balance is exactly 0
        assert_eq!(schedule[11].closing_balance, Decimal::ZERO);

        // Verify total payments are reducing
        assert!(schedule[0].total_payment > schedule[1].total_payment);
        assert!(schedule[10].total_payment > schedule[11].total_payment);
    }
}
