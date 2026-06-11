pub mod bom;
pub mod mrp;

#[derive(thiserror::Error, Debug)]
pub enum ManufacturingError {
    #[error("Database error: {0}")]
    Database(String),
    #[error("Cyclic BOM dependency detected")]
    CyclicDependency,
}
