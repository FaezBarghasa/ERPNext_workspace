use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use crate::registry::SchemaRegistry;
use crate::schema_compiler::SchemaCompiler;

pub struct MigrationEngine;

impl MigrationEngine {
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
                // Table does not exist, run full synchronization
                log::info!("Table {} does not exist. Compiling schema.", table_name);
                SchemaCompiler::synchronize_schema(db, ns, database, &doc_type).await?;
            } else {
                // Table exists, perform incremental migration
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

                // Add or update fields defined in the schema
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

                // Drop fields from DB that are no longer present in DynamicDocType fields (excluding 'id')
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{DynamicDocType, DynamicField, SurrealFieldType};
    use surrealdb::engine::any::connect;

    #[tokio::test]
    async fn test_run_migrations_incremental() {
        let db = connect("mem://").await.unwrap();
        let registry = SchemaRegistry::new();

        // 1. Initial version: User with name and email (required, unique)
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

        // Check if table is created and fields exist
        let mut res = db.query("INFO FOR TABLE tabUser;").await.unwrap();
        let val: Option<serde_json::Value> = res.take(0).unwrap();
        let fields = val.as_ref().unwrap().get("fields").unwrap().as_object().unwrap();
        assert!(fields.contains_key("name"));
        assert!(fields.contains_key("email"));
        let indexes = val.as_ref().unwrap().get("indexes").unwrap().as_object().unwrap();
        assert!(indexes.contains_key("email_idx"));

        // 2. Updated version: User with name and phone (email removed, phone added)
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

        // Check if phone was added and email was removed
        let mut res = db.query("INFO FOR TABLE tabUser;").await.unwrap();
        let val: Option<serde_json::Value> = res.take(0).unwrap();
        let fields = val.as_ref().unwrap().get("fields").unwrap().as_object().unwrap();
        assert!(fields.contains_key("name"));
        assert!(fields.contains_key("phone"));
        assert!(!fields.contains_key("email"));
    }
}
