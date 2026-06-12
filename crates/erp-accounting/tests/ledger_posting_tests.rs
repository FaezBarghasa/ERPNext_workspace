use erp_accounting::ledger_posting::{AccountingTransaction, LedgerPostingEngine};
use erp_accounting::posting::GLEntry;
use surrealdb::engine::any::connect;
use surrealdb::types::RecordId;
use rust_decimal_macros::dec;
use chrono::Utc;

#[tokio::test]
async fn test_balanced_transaction() {
    let db = connect("mem://").await.unwrap();
    db.use_ns("test").use_db("test").await.unwrap();

    let tx = AccountingTransaction {
        voucher_type: "Journal Entry".to_string(),
        voucher_no: "JV-001".to_string(),
        posting_date: Utc::now(),
        company: "Test Company".to_string(),
        remarks: None,
        entries: vec![
            GLEntry {
                account: RecordId::new("tabAccount", "Cash"),
                debit: dec!(100.0),
                credit: dec!(0.0),
                voucher_type: "Journal Entry".to_string(),
                voucher_no: "JV-001".to_string(),
                cost_center: None,
            },
            GLEntry {
                account: RecordId::new("tabAccount", "Sales"),
                debit: dec!(0.0),
                credit: dec!(100.0),
                voucher_type: "Journal Entry".to_string(),
                voucher_no: "JV-001".to_string(),
                cost_center: None,
            },
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
        posting_date: Utc::now(),
        company: "Test Company".to_string(),
        remarks: None,
        entries: vec![
            GLEntry {
                account: RecordId::new("tabAccount", "Cash"),
                debit: dec!(100.0),
                credit: dec!(0.0),
                voucher_type: "Journal Entry".to_string(),
                voucher_no: "JV-002".to_string(),
                cost_center: None,
            },
            GLEntry {
                account: RecordId::new("tabAccount", "Sales"),
                debit: dec!(0.0),
                credit: dec!(90.0),
                voucher_type: "Journal Entry".to_string(),
                voucher_no: "JV-002".to_string(),
                cost_center: None,
            },
        ],
    };

    let res = LedgerPostingEngine::commit_transaction(&db, &tx).await;
    assert!(res.is_err());
    let err_str = res.unwrap_err().to_string();
    assert!(err_str.contains("Imbalanced"));
}
