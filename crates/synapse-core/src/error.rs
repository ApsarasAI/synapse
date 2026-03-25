use thiserror::Error;

#[derive(Debug, Error)]
pub enum SynapseError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
}
