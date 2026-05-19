//! Order replacement workflow placeholder.

use serde_json::Value;

use crate::cli::ReplaceArgs;
use crate::error::AppError;

/// Executes `order replace`.
///
/// Full replacement support is implemented in a later refactor task. The
/// unified dispatcher calls this function so the command tree compiles now.
pub(crate) async fn execute_replace(_args: &ReplaceArgs) -> Result<Value, AppError> {
    Err(AppError::OrderValidation(
        "order replace is not implemented yet".to_string(),
    ))
}
