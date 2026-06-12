use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use surrealdb::types::RecordId;

/// A single General Ledger Entry line.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GLEntry {
    /// The account RecordId to post to.
    pub account: RecordId,
    /// Debit amount.
    pub debit: Decimal,
    /// Credit amount.
    pub credit: Decimal,
    /// Voucher type associated with the entry.
    pub voucher_type: String,
    /// Voucher number.
    pub voucher_no: String,
    /// Optional cost center.
    pub cost_center: Option<String>,
}

/// LedgerError represents all errors from ledger validations and postings.
#[derive(thiserror::Error, Debug)]
pub enum LedgerError {
    /// The transaction debits do not equal credits.
    #[error("Imbalanced transaction. Total debits: {debit}, Total credits: {credit}")]
    Imbalanced {
        /// Total sum of debits.
        debit: Decimal,
        /// Total sum of credits.
        credit: Decimal,
    },
    /// General database error.
    #[error("Database error: {0}")]
    Database(String),
    /// The transaction posting date lies within a closed or locked period.
    #[error("Posting date falls within a closed or locked fiscal period")]
    PeriodClosed,
    /// The entries contain invalid amounts (e.g. negative or all zero).
    #[error("Invalid transaction lines: all debits/credits must be non-negative and at least one must be non-zero")]
    InvalidEntries,
}

/// Validates entry lines to ensure they balance and have valid amounts.
pub fn validate_and_compile_transaction(entries: &[GLEntry]) -> Result<(), LedgerError> {
    let mut total_debit = Decimal::ZERO;
    let mut total_credit = Decimal::ZERO;
    let mut has_nonzero = false;

    for entry in entries {
        if entry.debit < Decimal::ZERO || entry.credit < Decimal::ZERO {
            return Err(LedgerError::InvalidEntries);
        }
        if !entry.debit.is_zero() || !entry.credit.is_zero() {
            has_nonzero = true;
        }
        total_debit += entry.debit;
        total_credit += entry.credit;
    }

    if !has_nonzero {
        return Err(LedgerError::InvalidEntries);
    }

    if total_debit != total_credit {
        return Err(LedgerError::Imbalanced {
            debit: total_debit,
            credit: total_credit,
        });
    }

    Ok(())
}
