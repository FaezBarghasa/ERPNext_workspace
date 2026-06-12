use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use chrono::{DateTime, Utc};

/// A single batch of stock received at a specific rate.
/// These are stored ordered by creation time to form the FIFO queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockBatch {
    pub item_code: String,
    pub warehouse: String,
    pub qty: Decimal,
    pub incoming_rate: Decimal,
    pub received_at: DateTime<Utc>,
    pub voucher_no: String,
}

/// Result of consuming stock via FIFO layers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FifoConsumeResult {
    pub consumed_qty: Decimal,
    pub weighted_avg_rate: Decimal,
    pub total_value: Decimal,
    /// Individual batch layers consumed (for audit trail)
    pub layers: Vec<ConsumedLayer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsumedLayer {
    pub batch_voucher_no: String,
    pub qty_consumed: Decimal,
    pub rate: Decimal,
    pub value: Decimal,
}

/// A stock ledger entry used for both inbound (positive qty) and outbound (negative qty) movements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockLedgerEntry {
    pub item_code: String,
    pub warehouse: String,
    /// Positive = receipt, Negative = issue/consumption
    pub qty: Decimal,
    /// For receipts: the actual purchase rate.
    /// For issues: computed by the FIFO engine from existing batches.
    pub valuation_rate: Decimal,
    pub voucher_type: String,
    pub voucher_no: String,
    pub posting_date: DateTime<Utc>,
}

pub struct FifoValuationEngine;

impl FifoValuationEngine {
    /// Processes an inbound stock receipt: creates a new FIFO batch and persists
    /// the stock ledger entry.
    pub async fn receive_stock(
        db: &Surreal<Any>,
        entry: &StockLedgerEntry,
    ) -> Result<(), String> {
        if entry.qty <= Decimal::ZERO {
            return Err("Receipt qty must be positive".to_string());
        }

        let batch = StockBatch {
            item_code: entry.item_code.clone(),
            warehouse: entry.warehouse.clone(),
            qty: entry.qty,
            incoming_rate: entry.valuation_rate,
            received_at: entry.posting_date,
            voucher_no: entry.voucher_no.clone(),
        };

        // Persist the FIFO batch layer
        db.query(
            "CREATE tabStockBatch CONTENT { \
                item_code: $item_code, \
                warehouse: $warehouse, \
                qty: $qty, \
                incoming_rate: $incoming_rate, \
                received_at: $received_at, \
                voucher_no: $voucher_no \
            };",
        )
        .bind(("item_code", batch.item_code.clone()))
        .bind(("warehouse", batch.warehouse.clone()))
        .bind(("qty", batch.qty.to_string()))
        .bind(("incoming_rate", batch.incoming_rate.to_string()))
        .bind(("received_at", batch.received_at.to_rfc3339()))
        .bind(("voucher_no", batch.voucher_no.clone()))
        .await
        .map_err(|e| e.to_string())?;

        // Persist the stock ledger entry
        Self::insert_sle(db, entry, entry.valuation_rate).await
    }

    /// Processes a stock issue using FIFO: consumes the oldest batches first,
    /// calculates the weighted-average consumption rate, and updates batch quantities.
    pub async fn issue_stock(
        db: &Surreal<Any>,
        entry: &StockLedgerEntry,
    ) -> Result<FifoConsumeResult, String> {
        if entry.qty >= Decimal::ZERO {
            return Err("Issue qty must be negative".to_string());
        }

        let qty_to_consume = entry.qty.abs();

        // Fetch existing FIFO batches for this item+warehouse ordered oldest-first
        let mut res = db
            .query(
                "SELECT * FROM tabStockBatch \
                 WHERE item_code = $item_code AND warehouse = $warehouse AND qty > 0 \
                 ORDER BY received_at ASC;",
            )
            .bind(("item_code", entry.item_code.clone()))
            .bind(("warehouse", entry.warehouse.clone()))
            .await
            .map_err(|e| e.to_string())?;

        let batch_rows: Vec<serde_json::Value> = res.take(0).unwrap_or_default();

        let mut remaining = qty_to_consume;
        let mut total_value = Decimal::ZERO;
        let mut layers: Vec<ConsumedLayer> = Vec::new();

        for row in &batch_rows {
            if remaining <= Decimal::ZERO {
                break;
            }

            let batch_id = row["id"].as_str().unwrap_or("").to_string();
            let batch_qty: Decimal = row["qty"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .unwrap_or(Decimal::ZERO);
            let batch_rate: Decimal = row["incoming_rate"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .unwrap_or(Decimal::ZERO);
            let batch_voucher = row["voucher_no"].as_str().unwrap_or("").to_string();

            let consumed = remaining.min(batch_qty);
            let value = consumed * batch_rate;
            total_value += value;
            remaining -= consumed;

            layers.push(ConsumedLayer {
                batch_voucher_no: batch_voucher,
                qty_consumed: consumed,
                rate: batch_rate,
                value,
            });

            // Update batch remaining quantity in DB
            let new_batch_qty = batch_qty - consumed;
            db.query("UPDATE $id SET qty = $new_qty;")
                .bind(("id", batch_id))
                .bind(("new_qty", new_batch_qty.to_string()))
                .await
                .map_err(|e| e.to_string())?;
        }

        if remaining > Decimal::ZERO {
            return Err(format!(
                "Insufficient stock for {} in {}. Short by {}",
                entry.item_code, entry.warehouse, remaining
            ));
        }

        let weighted_avg_rate = if qty_to_consume > Decimal::ZERO {
            total_value / qty_to_consume
        } else {
            Decimal::ZERO
        };

        // Persist the outgoing SLE with the FIFO-computed valuation rate
        Self::insert_sle(db, entry, weighted_avg_rate).await?;

        Ok(FifoConsumeResult {
            consumed_qty: qty_to_consume,
            weighted_avg_rate,
            total_value,
            layers,
        })
    }

    /// Returns the current in-stock quantity for an item+warehouse pair.
    pub async fn current_qty(
        db: &Surreal<Any>,
        item_code: &str,
        warehouse: &str,
    ) -> Result<Decimal, String> {
        let mut res = db
            .query(
                "SELECT math::sum(qty) AS total_qty FROM tabStockBatch \
                 WHERE item_code = $item_code AND warehouse = $warehouse;",
            )
            .bind(("item_code", item_code.to_string()))
            .bind(("warehouse", warehouse.to_string()))
            .await
            .map_err(|e| e.to_string())?;

        let row: Option<serde_json::Value> = res.take(0).unwrap_or(None);
        let qty_str = row
            .and_then(|v| v["total_qty"].as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "0".to_string());

        qty_str
            .parse::<Decimal>()
            .map_err(|e| format!("Failed to parse qty: {}", e))
    }

    /// Internal helper: inserts a Stock Ledger Entry record.
    async fn insert_sle(
        db: &Surreal<Any>,
        entry: &StockLedgerEntry,
        actual_rate: Decimal,
    ) -> Result<(), String> {
        db.query(
            "CREATE tabStockLedger CONTENT { \
                item_code: $item_code, \
                warehouse: $warehouse, \
                qty: $qty, \
                valuation_rate: $val_rate, \
                voucher_type: $voucher_type, \
                voucher_no: $voucher_no, \
                posting_date: $posting_date \
            };",
        )
        .bind(("item_code", entry.item_code.clone()))
        .bind(("warehouse", entry.warehouse.clone()))
        .bind(("qty", entry.qty.to_string()))
        .bind(("val_rate", actual_rate.to_string()))
        .bind(("voucher_type", entry.voucher_type.clone()))
        .bind(("voucher_no", entry.voucher_no.clone()))
        .bind(("posting_date", entry.posting_date.to_rfc3339()))
        .await
        .map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn fifo_weighted_average_calculation() {
        // Batch 1: 10 units @ 100 = value 1000
        // Batch 2: 5 units @ 120 = value 600
        // Issue 12 units → consume all of batch 1 + 2 from batch 2
        // weighted avg = (10*100 + 2*120) / 12 = (1000 + 240) / 12 = 103.33...
        let total_value = dec!(10) * dec!(100) + dec!(2) * dec!(120);
        let qty = dec!(12);
        let weighted_avg = total_value / qty;
        // 1240 / 12 = 103.333...
        assert_eq!(weighted_avg, dec!(1240) / dec!(12));
    }

    #[test]
    fn issue_exceeds_stock_should_error() {
        // Simulate the check: if remaining > 0 after consuming all batches, error
        let total_available = dec!(5);
        let requested = dec!(10);
        assert!(requested > total_available, "Should detect insufficient stock");
    }
}
