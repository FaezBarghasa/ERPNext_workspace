use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use crate::registry::SchemaRegistry;
use crate::schema_compiler::SchemaCompiler;
use crate::schema::{DocTypeSchema, SchemaError};

/// Database migration engine that compiles DocTypes and manages table sync schema-full updates.
pub struct MigrationEngine;

impl MigrationEngine {
    /// Runs a batch of migrations based on schemas registered in SchemaRegistry.
    pub async fn run_migrations(
        db: &Surreal<Any>,
        ns: &str,
        database: &str,
        registry: &SchemaRegistry,
    ) -> Result<(), surrealdb::Error> {
        db.use_ns(ns).use_db(database).await?;
        
        let doc_types = registry.get_all().await;
        
        let mut info_res = db.query("INFO FOR DB;").await?;
        let info_val: Option<serde_json::Value> = info_res.take(0)?;
        
        let empty_object = serde_json::Map::new();
        let db_tables = info_val
            .as_ref()
            .and_then(|v| v.get("tables"))
            .and_then(|t| t.as_object())
            .unwrap_or(&empty_object);

        for doc_type in doc_types {
            let table_name = format!("tab{}", doc_type.name);
            
            if !db_tables.contains_key(&table_name) {
                log::info!("Table {} does not exist. Compiling schema.", table_name);
                SchemaCompiler::synchronize_schema(db, ns, database, &doc_type).await?;
            } else {
                log::info!("Table {} exists. Verifying columns and indexes.", table_name);
                
                let mut table_info_res = db.query(format!("INFO FOR TABLE {};", table_name)).await?;
                let table_info_val: Option<serde_json::Value> = table_info_res.take(0)?;
                
                let db_fields = table_info_val
                    .as_ref()
                    .and_then(|v| v.get("fields"))
                    .and_then(|f| f.as_object())
                    .unwrap_or(&empty_object);
                    
                let db_indexes = table_info_val
                    .as_ref()
                    .and_then(|v| v.get("indexes"))
                    .and_then(|idx| idx.as_object())
                    .unwrap_or(&empty_object);

                for field in &doc_type.fields {
                    let req_clause = if field.required {
                        " ASSERT $value != NONE"
                    } else {
                        ""
                    };

                    if !db_fields.contains_key(&field.name) {
                        let define_field_query = format!(
                            "DEFINE FIELD {} ON {} TYPE {}{};",
                            field.name,
                            table_name,
                            field.field_type.to_surrealql(),
                            req_clause
                        );
                        db.query(&define_field_query).await?.check()?;
                    }

                    if field.unique {
                        let index_name = format!("{}_idx", field.name);
                        if !db_indexes.contains_key(&index_name) {
                            let define_index_query = format!(
                                "DEFINE INDEX {} ON {} COLUMNS {} UNIQUE;",
                                index_name,
                                table_name,
                                field.name
                            );
                            db.query(&define_index_query).await?.check()?;
                        }
                    }
                }

                for db_field in db_fields.keys() {
                    if db_field != "id" && !doc_type.fields.iter().any(|f| &f.name == db_field) {
                        log::info!("Removing field {} from table {}.", db_field, table_name);
                        let remove_field_query = format!("REMOVE FIELD {} ON {};", db_field, table_name);
                        db.query(&remove_field_query).await?.check()?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Incremental transactional schema diffing engine based on DocTypeSchema specs.
    pub async fn migrate_schema(
        db: &Surreal<Any>,
        ns: &str,
        database: &str,
        schema: &DocTypeSchema,
    ) -> Result<(), SchemaError> {
        schema.validate()?;
        db.use_ns(ns).use_db(database).await.map_err(|e| SchemaError::Database(e.to_string()))?;

        let table_name = format!("tab{}", schema.name);

        let mut info_res = db.query("INFO FOR DB;").await.map_err(|e| SchemaError::Database(e.to_string()))?;
        let info_val: Option<serde_json::Value> = info_res.take(0).map_err(|e| SchemaError::Database(e.to_string()))?;

        let empty_object = serde_json::Map::new();
        let db_tables = info_val
            .as_ref()
            .and_then(|v| v.get("tables"))
            .and_then(|t| t.as_object())
            .unwrap_or(&empty_object);

        let mut delta_queries = Vec::new();

        if !db_tables.contains_key(&table_name) {
            let creation_queries = SchemaCompiler::compile_to_surrealql(schema)?;
            delta_queries.extend(creation_queries);
        } else {
            let mut table_info_res = db.query(format!("INFO FOR TABLE {};", table_name)).await
                .map_err(|e| SchemaError::Database(e.to_string()))?;
            let table_info_val: Option<serde_json::Value> = table_info_res.take(0)
                .map_err(|e| SchemaError::Database(e.to_string()))?;

            let db_fields = table_info_val
                .as_ref()
                .and_then(|v| v.get("fields"))
                .and_then(|f| f.as_object())
                .unwrap_or(&empty_object);

            for field in &schema.fields {
                let field_type_sql = match field.fieldtype.as_str() {
                    "Data" => "string".to_string(),
                    "Int" => "int".to_string(),
                    "Float" | "Currency" => "number".to_string(),
                    "Link" => {
                        let target = field.options.as_ref().unwrap();
                        format!("record<tab{}>", target)
                    }
                    "Table" => {
                        let target = field.options.as_ref().unwrap();
                        format!("array<record<tab{}>>", target)
                    }
                    _ => return Err(SchemaError::Validation(format!("Unsupported type: {}", field.fieldtype))),
                };

                let mut expected_def = format!("DEFINE FIELD {} ON {} TYPE {}", field.fieldname, table_name, field_type_sql);
                if field.reqd {
                    expected_def.push_str(" ASSERT $value != NONE");
                }
                expected_def.push(';');

                if !db_fields.contains_key(&field.fieldname) {
                    delta_queries.push(expected_def);
                } else {
                    let current_def = db_fields.get(&field.fieldname).and_then(|v| v.as_str()).unwrap_or("");
                    let norm_curr = current_def.to_lowercase().replace(" ", "").replace(";", "").replace("permissionsfull", "");
                    let norm_exp = expected_def.to_lowercase().replace(" ", "").replace(";", "").replace("permissionsfull", "");
                    if norm_curr != norm_exp {
                        delta_queries.push(expected_def);
                    }
                }
            }

            for db_field in db_fields.keys() {
                if db_field != "id" && !schema.fields.iter().any(|f| &f.fieldname == db_field) {
                    delta_queries.push(format!("REMOVE FIELD {} ON {};", db_field, table_name));
                }
            }
        }

        if !delta_queries.is_empty() {
            let mut tx_query = String::new();
            tx_query.push_str("BEGIN TRANSACTION;\n");
            for query in delta_queries {
                tx_query.push_str(&query);
                tx_query.push_str("\n");
            }
            tx_query.push_str("COMMIT TRANSACTION;");

            db.query(tx_query).await
                .map_err(|e| SchemaError::Database(e.to_string()))?
                .check()
                .map_err(|e| SchemaError::Database(e.to_string()))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{DynamicDocType, DynamicField, SurrealFieldType, DocFieldSchema};
    use surrealdb::engine::any::connect;

    #[tokio::test]
    async fn test_run_migrations_incremental() {
        let db = connect("mem://").await.unwrap();
        let registry = SchemaRegistry::new();

        let doc_type_v1 = DynamicDocType {
            name: "User".to_string(),
            fields: vec![
                DynamicField {
                    name: "name".to_string(),
                    field_type: SurrealFieldType::String,
                    required: true,
                    unique: false,
                },
                DynamicField {
                    name: "email".to_string(),
                    field_type: SurrealFieldType::String,
                    required: true,
                    unique: true,
                },
            ],
            is_submittable: false,
            is_child_table: false,
        };

        registry.register(doc_type_v1).await;
        MigrationEngine::run_migrations(&db, "test", "test", &registry).await.unwrap();

        let mut res = db.query("INFO FOR TABLE tabUser;").await.unwrap();
        let val: Option<serde_json::Value> = res.take(0).unwrap();
        let fields = val.as_ref().unwrap().get("fields").unwrap().as_object().unwrap();
        assert!(fields.contains_key("name"));
        assert!(fields.contains_key("email"));
        let indexes = val.as_ref().unwrap().get("indexes").unwrap().as_object().unwrap();
        assert!(indexes.contains_key("email_idx"));

        let registry_v2 = SchemaRegistry::new();
        let doc_type_v2 = DynamicDocType {
            name: "User".to_string(),
            fields: vec![
                DynamicField {
                    name: "name".to_string(),
                    field_type: SurrealFieldType::String,
                    required: true,
                    unique: false,
                },
                DynamicField {
                    name: "phone".to_string(),
                    field_type: SurrealFieldType::String,
                    required: false,
                    unique: false,
                },
            ],
            is_submittable: false,
            is_child_table: false,
        };

        registry_v2.register(doc_type_v2).await;
        MigrationEngine::run_migrations(&db, "test", "test", &registry_v2).await.unwrap();

        let mut res = db.query("INFO FOR TABLE tabUser;").await.unwrap();
        let val: Option<serde_json::Value> = res.take(0).unwrap();
        let fields = val.as_ref().unwrap().get("fields").unwrap().as_object().unwrap();
        assert!(fields.contains_key("name"));
        assert!(fields.contains_key("phone"));
        assert!(!fields.contains_key("email"));
    }

    #[tokio::test]
    async fn test_migrate_schema_doctype_ast() {
        let db = connect("mem://").await.unwrap();
        
        let schema_v1 = DocTypeSchema {
            name: "Customer".to_string(),
            is_single: false,
            fields: vec![
                DocFieldSchema {
                    fieldname: "customer_name".to_string(),
                    fieldtype: "Data".to_string(),
                    reqd: true,
                    options: None,
                },
                DocFieldSchema {
                    fieldname: "credit_limit".to_string(),
                    fieldtype: "Currency".to_string(),
                    reqd: false,
                    options: None,
                },
            ],
            permissions: vec![],
        };

        MigrationEngine::migrate_schema(&db, "test", "test", &schema_v1).await.unwrap();

        let mut res = db.query("INFO FOR TABLE tabCustomer;").await.unwrap();
        let val: Option<serde_json::Value> = res.take(0).unwrap();
        let fields = val.as_ref().unwrap().get("fields").unwrap().as_object().unwrap();
        assert!(fields.contains_key("customer_name"));
        assert!(fields.contains_key("credit_limit"));

        let schema_v2 = DocTypeSchema {
            name: "Customer".to_string(),
            is_single: false,
            fields: vec![
                DocFieldSchema {
                    fieldname: "customer_name".to_string(),
                    fieldtype: "Data".to_string(),
                    reqd: true,
                    options: None,
                },
                DocFieldSchema {
                    fieldname: "outstanding_balance".to_string(),
                    fieldtype: "Currency".to_string(),
                    reqd: true,
                    options: None,
                },
            ],
            permissions: vec![],
        };

        MigrationEngine::migrate_schema(&db, "test", "test", &schema_v2).await.unwrap();

        let mut res = db.query("INFO FOR TABLE tabCustomer;").await.unwrap();
        let val: Option<serde_json::Value> = res.take(0).unwrap();
        let fields = val.as_ref().unwrap().get("fields").unwrap().as_object().unwrap();
        assert!(fields.contains_key("customer_name"));
        assert!(fields.contains_key("outstanding_balance"));
        assert!(!fields.contains_key("credit_limit"));
    }
}
