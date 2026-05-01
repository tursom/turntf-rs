use crate::errors::{Error, Result};
use crate::types::{DeliveryMode, SessionRef, UserRef};

const USER_METADATA_KEY_MAX_LENGTH: usize = 128;
const USER_METADATA_SCAN_LIMIT_MAX: i32 = 1000;

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

pub fn validate_session_ref(value: &SessionRef, field: &str) -> Result<()> {
    validate_positive_i64(value.serving_node_id, &format!("{field}.serving_node_id"))?;
    if value.session_id.trim().is_empty() {
        return Err(Error::validation(format!("{field}.session_id is required")));
    }
    Ok(())
}

pub fn validate_delivery_mode(mode: DeliveryMode) -> Result<()> {
    match mode {
        DeliveryMode::BestEffort | DeliveryMode::RouteRetry => Ok(()),
        DeliveryMode::Unspecified => Err(Error::validation("invalid delivery_mode \"\"")),
    }
}

pub fn validate_user_metadata_key(value: &str, field: &str) -> Result<()> {
    validate_user_metadata_key_fragment(value, field, false)
}

pub fn validate_optional_user_metadata_key_fragment(value: &str, field: &str) -> Result<()> {
    validate_user_metadata_key_fragment(value, field, true)
}

pub fn validate_user_metadata_scan_limit(limit: i32) -> Result<()> {
    if limit < 0 {
        return Err(Error::validation("limit must be positive"));
    }
    if limit > USER_METADATA_SCAN_LIMIT_MAX {
        return Err(Error::validation(format!(
            "limit cannot exceed {USER_METADATA_SCAN_LIMIT_MAX}"
        )));
    }
    Ok(())
}

fn validate_user_metadata_key_fragment(value: &str, field: &str, allow_empty: bool) -> Result<()> {
    if value.is_empty() {
        if allow_empty {
            return Ok(());
        }
        return Err(Error::validation(format!("{field} cannot be empty")));
    }
    if value.len() > USER_METADATA_KEY_MAX_LENGTH {
        return Err(Error::validation(format!(
            "{field} exceeds {USER_METADATA_KEY_MAX_LENGTH} characters"
        )));
    }
    for ch in value.chars() {
        match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '_' | ':' | '-' => {}
            _ => {
                return Err(Error::validation(format!(
                    "{field} contains unsupported character {ch:?}"
                )))
            }
        }
    }
    Ok(())
}
