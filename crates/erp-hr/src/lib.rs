pub mod payroll;
pub mod attendance;

#[derive(thiserror::Error, Debug)]
pub enum HrError {
    #[error("Validation error: {0}")]
    Validation(String),
}
