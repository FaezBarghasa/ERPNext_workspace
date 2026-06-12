use crate::schema::{DynamicDocType, DocTypeSchema, SchemaError};
use surrealdb::Surreal;
use surrealdb::engine::any::Any;

/// Compiler for schema structures to database-specific DDL syntax.
pub struct SchemaCompiler;

impl SchemaCompiler {
    /// Synchronizes a DynamicDocType schema directly into the database.
    pub async fn synchronize_schema(
        db: &Surreal<Any>,
        ns: &str,
        database: &str,
        doc_type: &DynamicDocType,
    ) -> Result<(), surrealdb::Error> {
        db.use_ns(ns).use_db(database).await?;

        let table_name = format!("tab{}", doc_type.name);

        let define_table_query = format!("DEFINE TABLE {} SCHEMAFULL;", table_name);
        db.query(&define_table_query).await?.check()?;

        for field in &doc_type.fields {
            let req_clause = if field.required {
                " ASSERT $value != NONE"
            } else {
                ""
            };

            let define_field_query = format!(
                "DEFINE FIELD {} ON {} TYPE {}{};",
                field.name,
                table_name,
                field.field_type.to_surrealql(),
                req_clause
            );
            db.query(&define_field_query).await?.check()?;

            if field.unique {
                let define_index_query = format!(
                    "DEFINE INDEX {}_idx ON {} COLUMNS {} UNIQUE;",
                    field.name,
                    table_name,
                    field.name
                );
                db.query(&define_index_query).await?.check()?;
            }
        }
        
        Ok(())
    }

    /// Compiles a DocTypeSchema definition directly to SurrealQL schema statements.
    pub fn compile_to_surrealql(schema: &DocTypeSchema) -> Result<Vec<String>, SchemaError> {
        schema.validate()?;
        
        let mut statements = Vec::new();
        let table_name = format!("tab{}", schema.name);
        
        statements.push(format!("DEFINE TABLE {} SCHEMAFULL;", table_name));
        
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
            
            let mut def = format!("DEFINE FIELD {} ON {} TYPE {}", field.fieldname, table_name, field_type_sql);
            if field.reqd {
                def.push_str(" ASSERT $value != NONE");
            }
            def.push(';');
            statements.push(def);
        }
        
        Ok(statements)
    }
}
