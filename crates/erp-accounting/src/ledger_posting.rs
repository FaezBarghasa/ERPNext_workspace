use serde::{Deserialize, Serialize};
use surrealdb::Surreal;
use surrealdb::engine::any::Any;

#[derive(Debug, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub account: String,
    pub debit: f64,
    pub credit: f64,
    pub cost_center: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountingTransaction {
    pub voucher_type: String,
    pub voucher_no: String,
    pub entries: Vec<LedgerEntry>,
}

pub struct LedgerPostingEngine;

impl LedgerPostingEngine {
    pub async fn commit_transaction(
        db: &Surreal<Any>,
        tx: &AccountingTransaction,
    ) -> Result<(), String> {
        let total_debit: f64 = tx.entries.iter().map(|e| e.debit).sum();
        let total_credit: f64 = tx.entries.iter().map(|e| e.credit).sum();

        if (total_debit - total_credit).abs() > 0.0001 {
            return Err(format!(
                "Imbalanced transaction. Debits: {}, Credits: {}",
                total_debit, total_credit
            ));
        }

        db.query("BEGIN TRANSACTION;").await.map_err(|e| e.to_string())?;

        for entry in &tx.entries {
            db.query(
                "CREATE tabGeneralLedger CONTENT {
                    voucher_type: $voucher_type,
                    voucher_no: $voucher_no,
                    account: $account,
                    debit: $debit,
                    credit: $credit,
                    cost_center: $cost_center
                };"
            )
            .bind(("voucher_type", tx.voucher_type.clone()))
            .bind(("voucher_no", tx.voucher_no.clone()))
            .bind(("account", entry.account.clone()))
            .bind(("debit", entry.debit))
            .bind(("credit", entry.credit))
            .bind(("cost_center", entry.cost_center.clone()))
            .await
            .map_err(|e| e.to_string())?;
        }

        db.query("COMMIT TRANSACTION;").await.map_err(|e| e.to_string())?;
        Ok(())
    }
}
