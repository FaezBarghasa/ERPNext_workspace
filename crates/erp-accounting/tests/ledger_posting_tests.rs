use erp_accounting::ledger_posting::{AccountingTransaction, LedgerEntry, LedgerPostingEngine};
use surrealdb::engine::any::connect;

#[tokio::test]
async fn test_balanced_transaction() {
    let db = connect("mem://").await.unwrap();
    db.use_ns("test").use_db("test").await.unwrap();

    let tx = AccountingTransaction {
        voucher_type: "Journal Entry".to_string(),
        voucher_no: "JV-001".to_string(),
        entries: vec![
            LedgerEntry { account: "Cash".to_string(), debit: 100.0, credit: 0.0, cost_center: None },
            LedgerEntry { account: "Sales".to_string(), debit: 0.0, credit: 100.0, cost_center: None },
        ],
    };

    let res = LedgerPostingEngine::commit_transaction(&db, &tx).await;
    assert!(res.is_ok());
}

#[tokio::test]
async fn test_imbalanced_transaction() {
    let db = connect("mem://").await.unwrap();
    db.use_ns("test").use_db("test").await.unwrap();

    let tx = AccountingTransaction {
        voucher_type: "Journal Entry".to_string(),
        voucher_no: "JV-002".to_string(),
        entries: vec![
            LedgerEntry { account: "Cash".to_string(), debit: 100.0, credit: 0.0, cost_center: None },
            LedgerEntry { account: "Sales".to_string(), debit: 0.0, credit: 90.0, cost_center: None },
        ],
    };

    let res = LedgerPostingEngine::commit_transaction(&db, &tx).await;
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("Imbalanced"));
}
