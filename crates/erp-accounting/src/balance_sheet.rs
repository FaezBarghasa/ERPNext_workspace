use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use surrealdb::Surreal;
use surrealdb::engine::any::Any;

/// Period-end balance summary for a single account.
#[derive(Debug, Serialize, Deserialize)]
pub struct AccountBalance {
    pub account: String,
    pub balance: Decimal,
}

pub struct BalanceSheetEngine;

impl BalanceSheetEngine {
    /// Returns the running balance (debit − credit) for a single account.
    pub async fn get_balance(
        db: &Surreal<Any>,
        account: &str,
        company: &str,
    ) -> Result<Decimal, String> {
        let mut res = db
            .query(
                "SELECT math::sum(debit) - math::sum(credit) AS balance \
                 FROM tabGeneralLedger \
                 WHERE account = $account AND company = $company \
                 GROUP ALL;",
            )
            .bind(("account", account.to_string()))
            .bind(("company", company.to_string()))
            .await
            .map_err(|e| e.to_string())?;

        let row: Option<serde_json::Value> = res.take(0).unwrap_or(None);
        let balance_str = row
            .and_then(|v| v["balance"].as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "0".to_string());

        balance_str
            .parse::<Decimal>()
            .map_err(|e| format!("Failed to parse balance: {}", e))
    }

    /// Returns balances for every account in a company, sorted by account name.
    pub async fn trial_balance(
        db: &Surreal<Any>,
        company: &str,
    ) -> Result<Vec<AccountBalance>, String> {
        let mut res = db
            .query(
                "SELECT account, math::sum(debit) - math::sum(credit) AS balance \
                 FROM tabGeneralLedger \
                 WHERE company = $company \
                 GROUP BY account \
                 ORDER BY account ASC;",
            )
            .bind(("company", company.to_string()))
            .await
            .map_err(|e| e.to_string())?;

        let rows: Vec<serde_json::Value> = res.take(0).unwrap_or_default();

        let mut balances = Vec::with_capacity(rows.len());
        for row in rows {
            let account = row["account"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            let balance: Decimal = row["balance"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .unwrap_or(Decimal::ZERO);
            balances.push(AccountBalance { account, balance });
        }
        Ok(balances)
    }
}
