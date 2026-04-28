use crate::errors::{Error, Result};
use crate::types::{DeliveryMode, UserRef};

pub fn validate_positive_i64(value: i64, field: &str) -> Result<()> {
    if value <= 0 {
        return Err(Error::validation(format!("{field} is required")));
    }
    Ok(())
}

pub fn validate_user_ref(value: &UserRef, field: &str) -> Result<()> {
    validate_positive_i64(value.node_id, &format!("{field}.node_id"))?;
    validate_positive_i64(value.user_id, &format!("{field}.user_id"))?;
    Ok(())
}

pub fn validate_delivery_mode(mode: DeliveryMode) -> Result<()> {
    match mode {
        DeliveryMode::BestEffort | DeliveryMode::RouteRetry => Ok(()),
        DeliveryMode::Unspecified => Err(Error::validation("invalid delivery_mode \"\"")),
    }
}
