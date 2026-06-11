use frappe_meta::schema::{DynamicDocType, DynamicField, SurrealFieldType};
use frappe_meta::schema_compiler::SchemaCompiler;
use surrealdb::engine::any::connect;

#[tokio::test]
async fn test_schema_compiler_invoice() {
    let db = connect("mem://").await.unwrap();
    db.use_ns("test_ns").use_db("test_db").await.unwrap();

    let invoice_schema = DynamicDocType {
        name: "SalesInvoice".to_string(),
        is_submittable: true,
        is_child_table: false,
        fields: vec![
            DynamicField {
                name: "customer".to_string(),
                field_type: SurrealFieldType::String,
                required: true,
                unique: false,
            },
            DynamicField {
                name: "total_amount".to_string(),
                field_type: SurrealFieldType::Float,
                required: true,
                unique: false,
            },
        ],
    };

    let result = SchemaCompiler::synchronize_schema(&db, "test_ns", "test_db", &invoice_schema).await;
    assert!(result.is_ok());

    // Test assertion by trying to insert blank values
    let insert_res = db.query("CREATE tabSalesInvoice CONTENT {};").await;
    println!("INSERT RES: {:#?}", insert_res);
    let err_msg = match insert_res {
        Ok(mut r) => {
            let val: Result<Option<serde_json::Value>, surrealdb::Error> = r.take(0);
            println!("TAKE RES: {:#?}", val);
            val.unwrap_err().to_string()
        }
        Err(e) => e.to_string(),
    };
    println!("ERR MSG: {}", err_msg);
    // An empty insert should fail because customer/total_amount are required
    assert!(!err_msg.is_empty(), "Insert should fail on assertion");

    // Test valid insert
    let valid_res = db.query("CREATE tabSalesInvoice CONTENT { customer: 'ACME', total_amount: 100.0 };").await;
    assert!(valid_res.is_ok());
}
