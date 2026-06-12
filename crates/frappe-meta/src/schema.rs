use serde::{Deserialize, Serialize};

/// Represents the database field type in SurrealDB schema mappings.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SurrealFieldType {
    /// String type.
    String,
    /// Integer type.
    Int,
    /// Floating point type.
    Float,
    /// Boolean type.
    Bool,
    /// Record reference type.
    Record(String),
    /// Array type.
    Array,
}

impl SurrealFieldType {
    /// Translates the type to a SurrealQL type definition.
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

/// A dynamic field defined on a DynamicDocType.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DynamicField {
    /// The name of the field.
    pub name: String,
    /// The database field type.
    pub field_type: SurrealFieldType,
    /// Whether this field is required.
    pub required: bool,
    /// Whether this field is unique.
    pub unique: bool,
}

/// A dynamic DocType schema definition used by the registry and existing migration flow.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DynamicDocType {
    /// The name of the DocType.
    pub name: String,
    /// The fields defined on this DocType.
    pub fields: Vec<DynamicField>,
    /// Whether the document can be submitted.
    pub is_submittable: bool,
    /// Whether this is a child table.
    pub is_child_table: bool,
}

/// SchemaError represents errors during schema compilation, validation, or migration.
#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    /// Input schema validation failed.
    #[error("Validation failed: {0}")]
    Validation(String),
    /// Database query or transaction failed.
    #[error("Database error: {0}")]
    Database(String),
}

/// Represents the field-level schema of a DocType.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocFieldSchema {
    /// The name of the field.
    pub fieldname: String,
    /// The type of the field (Data, Int, Float, Currency, Link, Table).
    pub fieldtype: String,
    /// Whether this field is required (not null).
    pub reqd: bool,
    /// Options, e.g. target table for Link/Table fields.
    pub options: Option<String>,
}

/// Represents permission metadata for a DocType.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PermissionMetadata {
    /// The user role.
    pub role: String,
    /// If read permission is granted.
    pub read: bool,
    /// If write permission is granted.
    pub write: bool,
}

/// Represents the complete AST schema definition of a DocType.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocTypeSchema {
    /// The name of the DocType.
    pub name: String,
    /// Whether this DocType represents a single document.
    pub is_single: bool,
    /// The fields within this DocType.
    pub fields: Vec<DocFieldSchema>,
    /// The permissions configuration.
    pub permissions: Vec<PermissionMetadata>,
}

impl DocFieldSchema {
    /// Validates field properties.
    pub fn validate(&self) -> Result<(), SchemaError> {
        if self.fieldname.is_empty() {
            return Err(SchemaError::Validation("Field name cannot be empty".to_string()));
        }
        if !self.fieldname.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Err(SchemaError::Validation(format!(
                "Field name '{}' contains invalid characters", self.fieldname
            )));
        }
        match self.fieldtype.as_str() {
            "Data" | "Int" | "Float" | "Currency" | "Link" | "Table" => {}
            _ => {
                return Err(SchemaError::Validation(format!(
                    "Unsupported field type '{}' for field '{}'", self.fieldtype, self.fieldname
                )));
            }
        }
        if self.fieldtype == "Link" && self.options.is_none() {
            return Err(SchemaError::Validation(format!(
                "Link field '{}' requires target options", self.fieldname
            )));
        }
        if self.fieldtype == "Table" && self.options.is_none() {
            return Err(SchemaError::Validation(format!(
                "Table field '{}' requires target options", self.fieldname
            )));
        }
        Ok(())
    }
}

impl DocTypeSchema {
    /// Validates the entire DocType schema.
    pub fn validate(&self) -> Result<(), SchemaError> {
        if self.name.is_empty() {
            return Err(SchemaError::Validation("DocType name cannot be empty".to_string()));
        }
        for field in &self.fields {
            field.validate()?;
        }
        Ok(())
    }
}
