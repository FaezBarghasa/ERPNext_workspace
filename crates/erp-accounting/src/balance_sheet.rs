use surrealdb::Surreal;
use surrealdb::engine::any::Any;

pub struct BalanceSheetEngine;

impl BalanceSheetEngine {
    pub async fn get_balance(db: &Surreal<Any>, account: &str) -> Result<f64, String> {
        let mut res = db.query(
            "SELECT math::sum(debit) - math::sum(credit) as balance FROM tabGeneralLedger WHERE account = $account GROUP BY account;"
        )
        .bind(("account", account.to_string()))
        .await
        .map_err(|e| e.to_string())?;

        let balance_obj: Option<serde_json::Value> = res.take(0).unwrap_or(None);
        if let Some(b) = balance_obj {
            Ok(b["balance"].as_f64().unwrap_or(0.0))
        } else {
            Ok(0.0)
        }
    }
}
