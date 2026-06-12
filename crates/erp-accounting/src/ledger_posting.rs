use crate::posting::{GLEntry, LedgerError, validate_and_compile_transaction};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use chrono::{DateTime, Utc};

/// Defines a locking period to prevent posting entries to closed accounting terms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeriodLock {
    /// Date range start.
    pub start_date: DateTime<Utc>,
    /// Date range end.
    pub end_date: DateTime<Utc>,
    /// Whether this period is locked.
    pub is_locked: bool,
}

/// A fully balanced double-entry accounting transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountingTransaction {
    /// Voucher type classification (e.g. Journal Entry).
    pub voucher_type: String,
    /// Unique voucher number identifier.
    pub voucher_no: String,
    /// Date of physical posting.
    pub posting_date: DateTime<Utc>,
    /// Scope company context.
    pub company: String,
    /// Context remarks.
    pub remarks: Option<String>,
    /// Set of entry lines comprising the transaction.
    pub entries: Vec<GLEntry>,
}

/// PostingReceipt generated upon successful ledger posting.
#[derive(Debug, Serialize, Deserialize)]
pub struct PostingReceipt {
    /// The processed voucher number.
    pub voucher_no: String,
    /// Total entry lines stored.
    pub entry_count: usize,
    /// Absolute total debit summed.
    pub total_debit: Decimal,
    /// Absolute total credit summed.
    pub total_credit: Decimal,
}

/// Ledger posting and transaction processor.
pub struct LedgerPostingEngine;

impl LedgerPostingEngine {
    /// Validates entries, checks locks, and posts entries atomically.
    pub async fn commit_transaction(
        db: &Surreal<Any>,
        tx: &AccountingTransaction,
    ) -> Result<PostingReceipt, LedgerError> {
        validate_and_compile_transaction(&tx.entries)?;

        let mut period_res = db
            .query("SELECT * FROM tabPeriodLock WHERE is_locked = true;")
            .await
            .map_err(|e| LedgerError::Database(e.to_string()))?;
        
        let locked_vals: Vec<serde_json::Value> = period_res.take(0).unwrap_or_default();
        let mut locked_periods = Vec::new();
        for val in locked_vals {
            let period: PeriodLock = serde_json::from_value(val)
                .map_err(|e| LedgerError::Database(e.to_string()))?;
            locked_periods.push(period);
        }

        for period in locked_periods {
            if tx.posting_date >= period.start_date && tx.posting_date <= period.end_date {
                return Err(LedgerError::PeriodClosed);
            }
        }

        let total_debit: Decimal = tx.entries.iter().map(|e| e.debit).sum();
        let total_credit: Decimal = tx.entries.iter().map(|e| e.credit).sum();

        let mut query_parts = Vec::with_capacity(tx.entries.len() + 2);
        query_parts.push("BEGIN TRANSACTION;".to_string());

        for (idx, entry) in tx.entries.iter().enumerate() {
            let entry_id = format!("{}_{}", tx.voucher_no, idx);
            let key_str = match &entry.account.key {
                surrealdb::types::RecordIdKey::String(s) => s.clone(),
                surrealdb::types::RecordIdKey::Number(n) => n.to_string(),
                _ => format!("{:?}", entry.account.key),
            };
            let account_str = format!("{}:{}", entry.account.table, key_str);
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
            .map_err(|e| LedgerError::Database(e.to_string()))?
            .check()
            .map_err(|e| LedgerError::Database(e.to_string()))?;

        Ok(PostingReceipt {
            voucher_no: tx.voucher_no.clone(),
            entry_count: tx.entries.len(),
            total_debit,
            total_credit,
        })
    }

    /// Generates cancellations mirror entries reversing transaction effects.
    pub async fn cancel_transaction(
        db: &Surreal<Any>,
        original: &AccountingTransaction,
    ) -> Result<PostingReceipt, LedgerError> {
        let cancel_tx = AccountingTransaction {
            voucher_type: original.voucher_type.clone(),
            voucher_no: format!("{}-CANCEL", original.voucher_no),
            posting_date: Utc::now(),
            company: original.company.clone(),
            remarks: Some(format!("Cancellation of {}", original.voucher_no)),
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
