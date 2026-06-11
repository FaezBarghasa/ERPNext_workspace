use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use surrealdb::types::RecordId;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GLEntry {
    pub account: RecordId,
    pub debit: Decimal,
    pub credit: Decimal,
    pub voucher_type: String,
    pub voucher_no: String,
    pub cost_center: Option<String>,
}

#[derive(thiserror::Error, Debug)]
pub enum LedgerError {
    #[error("Imbalanced transaction. Total debits: {debit}, Total credits: {credit}")]
    Imbalanced { debit: Decimal, credit: Decimal },
    #[error("Database error: {0}")]
    Database(String),
}

pub fn validate_and_compile_transaction(entries: &[GLEntry]) -> Result<(), LedgerError> {
    let total_debit: Decimal = entries.iter().map(|e| e.debit).sum();
    let total_credit: Decimal = entries.iter().map(|e| e.credit).sum();

    if total_debit != total_credit {
        return Err(LedgerError::Imbalanced {
            debit: total_debit,
            credit: total_credit,
        });
    }
    Ok(())
}
