use crate::schema::DynamicDocType;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;

pub struct SchemaCompiler;

impl SchemaCompiler {
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
}
