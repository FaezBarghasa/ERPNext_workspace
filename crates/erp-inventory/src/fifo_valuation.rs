use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use surrealdb::types::RecordId;
use chrono::{DateTime, Utc};

/// ValuationError represents issues occurring during FIFO inventory calculation and database updates.
#[derive(Debug, thiserror::Error)]
pub enum ValuationError {
    /// The requested quantity for dispatch exceeds current physical warehouse stock.
    #[error("Insufficient stock. Requested: {requested}, Available: {available}")]
    InsufficientStock {
        /// The quantity requested.
        requested: Decimal,
        /// The current stock level.
        available: Decimal,
    },
    /// A general database error.
    #[error("Database error: {0}")]
    Database(String),
}

/// A single stock item layer in the FIFO queue.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StockStackItem {
    /// The optional database RecordId of the corresponding batch record.
    pub id: Option<RecordId>,
    /// Available quantity.
    pub qty: Decimal,
    /// Incoming valuation rate.
    pub rate: Decimal,
    /// Exact receipt timestamp.
    pub entry_time: DateTime<Utc>,
}

/// Consumes quantity from the stock stack oldest-first, returning the total cost of goods sold.
pub fn consume_fifo(
    stack: &mut Vec<StockStackItem>,
    mut dispatch_qty: Decimal,
) -> Result<Decimal, ValuationError> {
    let total_available: Decimal = stack.iter().map(|item| item.qty).sum();
    if dispatch_qty > total_available {
        return Err(ValuationError::InsufficientStock {
            requested: dispatch_qty,
            available: total_available,
        });
    }

    let mut total_cost = Decimal::ZERO;
    stack.sort_by_key(|item| item.entry_time);

    while dispatch_qty > Decimal::ZERO && !stack.is_empty() {
        let mut item = stack.remove(0);
        let consumed = dispatch_qty.min(item.qty);
        let item_cost = consumed * item.rate;
        total_cost += item_cost;
        dispatch_qty -= consumed;
        item.qty -= consumed;

        if item.qty > Decimal::ZERO {
            stack.insert(0, item);
        }
    }

    Ok(total_cost.round_dp_with_strategy(6, RoundingStrategy::MidpointAwayFromZero))
}

/// Valuation and ledger processing engine.
pub struct FifoValuationEngine;

impl FifoValuationEngine {
    /// Processes stock issues, calculates total COGS, and updates database records inside a transaction.
    pub async fn process_stock_issue(
        db: &Surreal<Any>,
        item_code: &str,
        warehouse: &str,
        dispatch_qty: Decimal,
        voucher_no: &str,
        posting_date: DateTime<Utc>,
    ) -> Result<Decimal, ValuationError> {
        let mut res = db
            .query("SELECT * FROM tabStockBatch WHERE item_code = $item_code AND warehouse = $warehouse AND qty > 0 ORDER BY received_at ASC;")
            .bind(("item_code", item_code.to_string()))
            .bind(("warehouse", warehouse.to_string()))
            .await
            .map_err(|e| ValuationError::Database(e.to_string()))?;
        
        let batch_vals: Vec<serde_json::Value> = res.take(0).unwrap_or_default();
        println!("BATCH_VALS FOR {} IN {}: {:?}", item_code, warehouse, batch_vals);
        
        let mut stack = Vec::new();
        for val in batch_vals {
            let id_val = val["id"].clone();
            let id: RecordId = if let Some(s) = id_val.as_str() {
                let parts: Vec<&str> = s.split(':').collect();
                if parts.len() == 2 {
                    RecordId::new(parts[0], parts[1])
                } else {
                    return Err(ValuationError::Database("Invalid RecordId string format".to_string()));
                }
            } else {
                serde_json::from_value(id_val)
                    .map_err(|e| ValuationError::Database(e.to_string()))?
            };
            let qty: Decimal = if let Some(n) = val["qty"].as_f64() {
                Decimal::from_f64_retain(n).unwrap_or(Decimal::ZERO)
            } else if let Some(s) = val["qty"].as_str() {
                s.parse().unwrap_or(Decimal::ZERO)
            } else {
                Decimal::ZERO
            };
            let rate: Decimal = if let Some(n) = val["incoming_rate"].as_f64() {
                Decimal::from_f64_retain(n).unwrap_or(Decimal::ZERO)
            } else if let Some(s) = val["incoming_rate"].as_str() {
                s.parse().unwrap_or(Decimal::ZERO)
            } else {
                Decimal::ZERO
            };
            let entry_time: DateTime<Utc> = serde_json::from_value(val["received_at"].clone())
                .map_err(|e| ValuationError::Database(e.to_string()))?;
            
            stack.push(StockStackItem {
                id: Some(id),
                qty,
                rate,
                entry_time,
            });
        }

        let mut stack_clone = stack.clone();
        let total_cost = consume_fifo(&mut stack_clone, dispatch_qty)?;

        let mut query_parts = Vec::new();
        query_parts.push("BEGIN TRANSACTION;".to_string());

        for original in &stack {
            let id = original.id.as_ref().unwrap();
            let id_str = format!("{}:`{}`", id.table, match &id.key {
                surrealdb::types::RecordIdKey::String(s) => s.clone(),
                surrealdb::types::RecordIdKey::Number(n) => n.to_string(),
                _ => format!("{:?}", id.key),
            });

            if let Some(remaining) = stack_clone.iter().find(|item| item.id == original.id) {
                if remaining.qty != original.qty {
                    query_parts.push(format!(
                        "UPDATE {} SET qty = {};",
                        id_str,
                        remaining.qty
                    ));
                }
            } else {
                query_parts.push(format!(
                    "UPDATE {} SET qty = 0;",
                    id_str
                ));
            }
        }

        let actual_rate = if dispatch_qty.is_zero() { Decimal::ZERO } else { total_cost / dispatch_qty };

        query_parts.push(format!(
            "CREATE tabStockLedger CONTENT {{ \
                item_code: \"{item_code}\", \
                warehouse: \"{warehouse}\", \
                qty: {qty}, \
                valuation_rate: {val_rate}, \
                voucher_type: \"Stock Issue\", \
                voucher_no: \"{voucher_no}\", \
                posting_date: \"{posting_date}\" \
            }};",
            item_code = item_code,
            warehouse = warehouse,
            qty = -dispatch_qty,
            val_rate = actual_rate,
            voucher_no = voucher_no,
            posting_date = posting_date.to_rfc3339()
        ));

        query_parts.push("COMMIT TRANSACTION;".to_string());
        let full_query = query_parts.join("\n");

        db.query(&full_query)
            .await
            .map_err(|e| ValuationError::Database(e.to_string()))?
            .check()
            .map_err(|e| ValuationError::Database(e.to_string()))?;

        Ok(total_cost)
    }

    /// Creates a new receipt batch layer and persists a ledger entry.
    pub async fn process_stock_receipt(
        db: &Surreal<Any>,
        item_code: &str,
        warehouse: &str,
        qty: Decimal,
        rate: Decimal,
        voucher_no: &str,
        posting_date: DateTime<Utc>,
    ) -> Result<(), ValuationError> {
        let mut query_parts = Vec::new();
        query_parts.push("BEGIN TRANSACTION;".to_string());

        query_parts.push(format!(
            "CREATE tabStockBatch CONTENT {{ \
                item_code: \"{item_code}\", \
                warehouse: \"{warehouse}\", \
                qty: {qty}, \
                incoming_rate: {rate}, \
                received_at: \"{posting_date}\", \
                voucher_no: \"{voucher_no}\" \
            }};",
            item_code = item_code,
            warehouse = warehouse,
            qty = qty,
            rate = rate,
            posting_date = posting_date.to_rfc3339(),
            voucher_no = voucher_no
        ));

        query_parts.push(format!(
            "CREATE tabStockLedger CONTENT {{ \
                item_code: \"{item_code}\", \
                warehouse: \"{warehouse}\", \
                qty: {qty}, \
                valuation_rate: {rate}, \
                voucher_type: \"Stock Receipt\", \
                voucher_no: \"{voucher_no}\", \
                posting_date: \"{posting_date}\" \
            }};",
            item_code = item_code,
            warehouse = warehouse,
            qty = qty,
            rate = rate,
            voucher_no = voucher_no,
            posting_date = posting_date.to_rfc3339()
        ));

        query_parts.push("COMMIT TRANSACTION;".to_string());
        let full_query = query_parts.join("\n");

        db.query(&full_query)
            .await
            .map_err(|e| ValuationError::Database(e.to_string()))?
            .check()
            .map_err(|e| ValuationError::Database(e.to_string()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use surrealdb::engine::any::connect;

    #[test]
    fn test_consume_fifo_basic() {
        let mut stack = vec![
            StockStackItem {
                id: None,
                qty: dec!(10.0),
                rate: dec!(10.0),
                entry_time: Utc::now() - chrono::Duration::hours(2),
            },
            StockStackItem {
                id: None,
                qty: dec!(5.0),
                rate: dec!(12.0),
                entry_time: Utc::now() - chrono::Duration::hours(1),
            },
        ];

        let cost = consume_fifo(&mut stack, dec!(12.0)).unwrap();
        assert_eq!(cost, dec!(124.0));
        assert_eq!(stack.len(), 1);
        assert_eq!(stack[0].qty, dec!(3.0));
    }

    #[tokio::test]
    async fn test_fifo_three_batches_dispatch() {
        let db = connect("mem://").await.unwrap();
        db.use_ns("test").use_db("test").await.unwrap();

        let item = "ITEM-001";
        let wh = "Warehouse-A";

        FifoValuationEngine::process_stock_receipt(&db, item, wh, dec!(10.0), dec!(10.0), "REC-01", Utc::now() - chrono::Duration::hours(3))
            .await
            .unwrap();

        FifoValuationEngine::process_stock_receipt(&db, item, wh, dec!(20.0), dec!(12.0), "REC-02", Utc::now() - chrono::Duration::hours(2))
            .await
            .unwrap();

        FifoValuationEngine::process_stock_receipt(&db, item, wh, dec!(5.0), dec!(15.0), "REC-03", Utc::now() - chrono::Duration::hours(1))
            .await
            .unwrap();

        let cogs = FifoValuationEngine::process_stock_issue(&db, item, wh, dec!(22.0), "ISS-01", Utc::now())
            .await
            .unwrap();

        assert_eq!(cogs, dec!(244.0));

        let mut res = db.query("SELECT * FROM tabStockBatch WHERE item_code = $item AND warehouse = $wh AND qty > 0 ORDER BY received_at ASC;")
            .bind(("item", item.to_string()))
            .bind(("wh", wh.to_string()))
            .await
            .unwrap();
        let remaining_batches: Vec<serde_json::Value> = res.take(0).unwrap();
        let mut total_val = dec!(0.0);
        for val in remaining_batches {
            let qty: Decimal = if let Some(n) = val["qty"].as_f64() {
                Decimal::from_f64_retain(n).unwrap()
            } else {
                val["qty"].as_str().unwrap().parse().unwrap()
            };
            let rate: Decimal = if let Some(n) = val["incoming_rate"].as_f64() {
                Decimal::from_f64_retain(n).unwrap()
            } else {
                val["incoming_rate"].as_str().unwrap().parse().unwrap()
            };
            total_val += qty * rate;
        }
        assert_eq!(total_val, dec!(171.0));
    }
}
