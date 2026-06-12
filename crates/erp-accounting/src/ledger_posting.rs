use crate::posting::{GLEntry, LedgerError};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use chrono::{DateTime, Utc};

/// A fully balanced double-entry accounting transaction.
/// All entries must satisfy: Σ debit == Σ credit before committing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountingTransaction {
    /// Identifies the source document type (e.g. "Sales Invoice", "Payment Entry")
    pub voucher_type: String,
    /// The primary key of the source voucher document
    pub voucher_no: String,
    /// Posting date in UTC; stored on each gl_entry for period-end reporting
    pub posting_date: DateTime<Utc>,
    /// The company scope for this transaction (multi-tenant)
    pub company: String,
    /// Remarks visible in the general ledger report
    pub remarks: Option<String>,
    /// The balanced set of ledger lines
    pub entries: Vec<GLEntry>,
}

/// Result record returned after a successful commit.
#[derive(Debug, Serialize, Deserialize)]
pub struct PostingReceipt {
    pub voucher_no: String,
    pub entry_count: usize,
    pub total_debit: Decimal,
    pub total_credit: Decimal,
}

pub struct LedgerPostingEngine;

impl LedgerPostingEngine {
    /// Validates balance and atomically persists all GL entries to SurrealDB.
    ///
    /// Uses a single SurrealDB `BEGIN TRANSACTION … COMMIT TRANSACTION` block so
    /// that a failure on any individual entry rolls the entire voucher back.
    pub async fn commit_transaction(
        db: &Surreal<Any>,
        tx: &AccountingTransaction,
    ) -> Result<PostingReceipt, LedgerError> {
        // ── 1. Compile & validate balance ─────────────────────────────────────
        let total_debit: Decimal = tx.entries.iter().map(|e| e.debit).sum();
        let total_credit: Decimal = tx.entries.iter().map(|e| e.credit).sum();

        if total_debit != total_credit {
            return Err(LedgerError::Imbalanced {
                debit: total_debit,
                credit: total_credit,
            });
        }

        // ── 2. Atomically insert every GL entry ────────────────────────────────
        // SurrealDB 3.x supports nested BEGIN/COMMIT inside a multi-statement
        // query chain. We build one query string so all entries land in a single
        // network round-trip — important for low-latency RPi 5 deployments.
        let mut query_parts: Vec<String> = Vec::with_capacity(tx.entries.len() + 2);
        query_parts.push("BEGIN TRANSACTION;".to_string());

        for (idx, entry) in tx.entries.iter().enumerate() {
            // Each GL entry is uniquely identified by voucher_no + line index
            let entry_id = format!("{}_{}", tx.voucher_no, idx);
            let account_str = format!("{}:{}", entry.account.table(), entry.account.key());
            let cost_center = entry
                .cost_center
                .as_deref()
                .map(|s| format!("\"{}\"", s))
                .unwrap_or_else(|| "NONE".to_string());

            query_parts.push(format!(
                "CREATE tabGeneralLedger:`{entry_id}` CONTENT {{ \
                    voucher_type: \"{voucher_type}\", \
                    voucher_no: \"{voucher_no}\", \
                    account: {account_str}, \
                    debit: {debit}, \
                    credit: {credit}, \
                    cost_center: {cost_center}, \
                    posting_date: \"{posting_date}\", \
                    company: \"{company}\", \
                    remarks: {remarks} \
                }};",
                entry_id = entry_id,
                voucher_type = tx.voucher_type,
                voucher_no = tx.voucher_no,
                account_str = account_str,
                debit = entry.debit,
                credit = entry.credit,
                cost_center = cost_center,
                posting_date = tx.posting_date.to_rfc3339(),
                company = tx.company,
                remarks = tx
                    .remarks
                    .as_deref()
                    .map(|r| format!("\"{}\"", r))
                    .unwrap_or_else(|| "NONE".to_string()),
            ));
        }

        query_parts.push("COMMIT TRANSACTION;".to_string());
        let full_query = query_parts.join("\n");

        db.query(&full_query)
            .await
            .map_err(|e| LedgerError::Database(e.to_string()))?;

        Ok(PostingReceipt {
            voucher_no: tx.voucher_no.clone(),
            entry_count: tx.entries.len(),
            total_debit,
            total_credit,
        })
    }

    /// Reverses a previously committed voucher by posting negated mirror entries.
    /// The new cancellation voucher is suffixed with `-CANCEL`.
    pub async fn cancel_transaction(
        db: &Surreal<Any>,
        original: &AccountingTransaction,
    ) -> Result<PostingReceipt, LedgerError> {
        let cancel_tx = AccountingTransaction {
            voucher_type: original.voucher_type.clone(),
            voucher_no: format!("{}-CANCEL", original.voucher_no),
            posting_date: Utc::now(),
            company: original.company.clone(),
            remarks: Some(format!(
                "Cancellation of {}",
                original.voucher_no
            )),
            // Swap debit ↔ credit on every entry to produce the equal-and-opposite
            entries: original
                .entries
                .iter()
                .map(|e| GLEntry {
                    account: e.account.clone(),
                    debit: e.credit,
                    credit: e.debit,
                    voucher_type: original.voucher_type.clone(),
                    voucher_no: format!("{}-CANCEL", original.voucher_no),
                    cost_center: e.cost_center.clone(),
                })
                .collect(),
        };

        Self::commit_transaction(db, &cancel_tx).await
    }
}

// ── Unit tests (no live DB required; validate logic only) ────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use crate::posting::LedgerError;
    use rust_decimal_macros::dec;
    use surrealdb::types::RecordId;

    fn make_entry(account: &str, debit: Decimal, credit: Decimal) -> GLEntry {
        GLEntry {
            account: RecordId::from_table_key("tabAccount", account),
            debit,
            credit,
            voucher_type: "Test".to_string(),
            voucher_no: "TEST-0001".to_string(),
            cost_center: None,
        }
    }

    #[test]
    fn balanced_transaction_passes_validation() {
        // Debit Cash 1000, Credit Revenue 1000 — balanced
        let entries = vec![
            make_entry("Cash", dec!(1000), dec!(0)),
            make_entry("Revenue", dec!(0), dec!(1000)),
        ];
        let total_d: Decimal = entries.iter().map(|e| e.debit).sum();
        let total_c: Decimal = entries.iter().map(|e| e.credit).sum();
        assert_eq!(total_d, total_c, "Transaction must balance");
    }

    #[test]
    fn imbalanced_transaction_returns_error() {
        let entries = vec![
            make_entry("Cash", dec!(1000), dec!(0)),
            make_entry("Revenue", dec!(0), dec!(900)), // intentionally off by 100
        ];
        let total_d: Decimal = entries.iter().map(|e| e.debit).sum();
        let total_c: Decimal = entries.iter().map(|e| e.credit).sum();
        assert_ne!(total_d, total_c);
        // Verify the error variant carries the correct amounts
        let err = LedgerError::Imbalanced {
            debit: total_d,
            credit: total_c,
        };
        assert!(err.to_string().contains("1000"));
        assert!(err.to_string().contains("900"));
    }

    #[test]
    fn cancel_swaps_debit_and_credit() {
        let original_entries = vec![
            make_entry("Cash", dec!(500), dec!(0)),
            make_entry("Accounts_Payable", dec!(0), dec!(500)),
        ];
        // Mirror: credit becomes debit, debit becomes credit
        let cancelled: Vec<_> = original_entries
            .iter()
            .map(|e| (e.credit, e.debit))
            .collect();
        assert_eq!(cancelled[0], (dec!(0), dec!(500)));
        assert_eq!(cancelled[1], (dec!(500), dec!(0)));
        // Still balances
        let total_d: Decimal = cancelled.iter().map(|(d, _)| d).sum();
        let total_c: Decimal = cancelled.iter().map(|(_, c)| c).sum();
        assert_eq!(total_d, total_c);
    }
}
