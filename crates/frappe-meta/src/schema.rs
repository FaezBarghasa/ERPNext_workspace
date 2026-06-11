use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SurrealFieldType {
    String,
    Int,
    Float,
    Bool,
    Record(String),
    Array,
}

impl SurrealFieldType {
    pub fn to_surrealql(&self) -> String {
        match self {
            SurrealFieldType::String => "string".to_string(),
            SurrealFieldType::Int => "int".to_string(),
            SurrealFieldType::Float => "float".to_string(),
            SurrealFieldType::Bool => "bool".to_string(),
            SurrealFieldType::Record(table) => format!("record<{}>", table),
            SurrealFieldType::Array => "array".to_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DynamicField {
    pub name: String,
    pub field_type: SurrealFieldType,
    pub required: bool,
    pub unique: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DynamicDocType {
    pub name: String,
    pub fields: Vec<DynamicField>,
    pub is_submittable: bool,
    pub is_child_table: bool,
}
