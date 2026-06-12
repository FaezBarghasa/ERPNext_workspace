use crate::ledger_posting::{AccountingTransaction, LedgerPostingEngine};
use crate::posting::{GLEntry, LedgerError};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use surrealdb::types::RecordId;
use chrono::{DateTime, Utc};

/// Represents a fiscal year period for closing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FiscalYear {
    pub name: String,
    pub year_start: DateTime<Utc>,
    pub year_end: DateTime<Utc>,
}

pub struct PeriodClosingEngine;

impl PeriodClosingEngine {
    /// Closes a fiscal year by transferring all P&L account balances
    /// into the Retained Earnings account, producing a balanced GL entry.
    ///
    /// Algorithm:
    ///   1. Fetch all revenue & expense account balances for the period.
    ///   2. Calculate net profit/loss: net = Σ revenue_credits - Σ expense_debits
    ///   3. Zero-out each P&L account.
    ///   4. Book the net to Retained Earnings.
    pub async fn close_fiscal_year(
        db: &Surreal<Any>,
        fiscal_year: &FiscalYear,
        company: &str,
        retained_earnings_account: &str,
    ) -> Result<(), LedgerError> {
        // Query all P&L accounts (Income & Expense) with non-zero balances
        let mut res = db
            .query(
                "SELECT account, \
                    math::sum(debit) AS total_debit, \
                    math::sum(credit) AS total_credit \
                 FROM tabGeneralLedger \
                 WHERE company = $company \
                   AND posting_date >= $start \
                   AND posting_date <= $end \
                   AND account_type IN [\"Income\", \"Expense\"] \
                 GROUP BY account;",
            )
            .bind(("company", company.to_string()))
            .bind(("start", fiscal_year.year_start.to_rfc3339()))
            .bind(("end", fiscal_year.year_end.to_rfc3339()))
            .await
            .map_err(|e| LedgerError::Database(e.to_string()))?;

        let rows: Vec<serde_json::Value> = res.take(0).unwrap_or_default();

        if rows.is_empty() {
            // Nothing to close
            return Ok(());
        }

        let mut closing_entries: Vec<GLEntry> = Vec::new();
        let mut net_profit = Decimal::ZERO;

        for row in &rows {
            let account_str = row["account"].as_str().unwrap_or("").to_string();
            let total_debit: Decimal = row["total_debit"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .unwrap_or(Decimal::ZERO);
            let total_credit: Decimal = row["total_credit"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .unwrap_or(Decimal::ZERO);

            let net = total_credit - total_debit;
            net_profit += net;

            // To zero the account, post the opposite of its running balance
            let (debit, credit) = if net > Decimal::ZERO {
                // Credit-heavy (income) → debit to close
                (net.abs(), dec!(0))
            } else {
                // Debit-heavy (expense) → credit to close
                (dec!(0), net.abs())
            };

            closing_entries.push(GLEntry {
                account: RecordId::new("tabAccount", account_str.as_str()),
                debit,
                credit,
                voucher_type: "Period Closing Voucher".to_string(),
                voucher_no: format!("PCV-{}", fiscal_year.name),
                cost_center: None,
            });
        }

        // Book net profit/loss to Retained Earnings to balance the transaction
        let (re_debit, re_credit) = if net_profit >= Decimal::ZERO {
            // Net profit → credit Retained Earnings
            (dec!(0), net_profit)
        } else {
            // Net loss → debit Retained Earnings
            (net_profit.abs(), dec!(0))
        };

        closing_entries.push(GLEntry {
            account: RecordId::new(
                "tabAccount",
                retained_earnings_account,
            ),
            debit: re_debit,
            credit: re_credit,
            voucher_type: "Period Closing Voucher".to_string(),
            voucher_no: format!("PCV-{}", fiscal_year.name),
            cost_center: None,
        });

        let closing_tx = AccountingTransaction {
            voucher_type: "Period Closing Voucher".to_string(),
            voucher_no: format!("PCV-{}", fiscal_year.name),
            posting_date: Utc::now(),
            company: company.to_string(),
            remarks: Some(format!(
                "Automatic period closing for fiscal year {}",
                fiscal_year.name
            )),
            entries: closing_entries,
        };

        LedgerPostingEngine::commit_transaction(db, &closing_tx).await?;
        Ok(())
    }
}
