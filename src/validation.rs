use crate::errors::{Error, Result};
use crate::types::{DeliveryMode, SessionRef, UserRef};

/// 用户元数据键的最大长度（128 字符）。
const USER_METADATA_KEY_MAX_LENGTH: usize = 128;

/// 用户元数据扫描的最大限制数（1000 条）。
const USER_METADATA_SCAN_LIMIT_MAX: i32 = 1000;

pub(crate) fn normalize_login_name(value: &str) -> String {
    value.trim().to_owned()
}

/// 验证 i64 值必须为正数。
///
/// 用于验证节点 ID、用户 ID 等必须为正整数的参数。
///
/// # 参数
/// - `value` - 要验证的 i64 值
/// - `field` - 字段名称，用于错误消息中的定位
///
/// # Errors
/// 如果值小于等于 0，返回 `Error::Validation`。
pub fn validate_positive_i64(value: i64, field: &str) -> Result<()> {
    if value <= 0 {
        return Err(Error::validation(format!("{field} is required")));
    }
    Ok(())
}

/// 验证登录名不能为空。
///
/// # 参数
/// - `value` - 登录名字符串
/// - `field` - 字段名称
///
/// # Errors
/// 如果去除首尾空格后的登录名为空，返回 `Error::Validation`。
pub fn validate_login_name(value: &str, field: &str) -> Result<()> {
    if normalize_login_name(value).is_empty() {
        return Err(Error::validation(format!("{field} is required")));
    }
    Ok(())
}

/// 验证用户引用（UserRef）的 node_id 和 user_id 都为正数。
///
/// # 参数
/// - `value` - 用户引用
/// - `field` - 字段名称
///
/// # Errors
/// 如果 `node_id` 或 `user_id` 小于等于 0，返回 `Error::Validation`。
pub fn validate_user_ref(value: &UserRef, field: &str) -> Result<()> {
    validate_positive_i64(value.node_id, &format!("{field}.node_id"))?;
    validate_positive_i64(value.user_id, &format!("{field}.user_id"))?;
    Ok(())
}

/// 验证可选用户引用。
///
/// 当 `node_id` 和 `user_id` 都为 0 时，视为“未设置”，返回 `Ok(false)`。
/// 其余情况下要求两个字段都为正数，返回 `Ok(true)`。
pub(crate) fn validate_optional_user_ref(value: &UserRef, field: &str) -> Result<bool> {
    if value.node_id == 0 && value.user_id == 0 {
        return Ok(false);
    }
    validate_user_ref(value, field)?;
    Ok(true)
}

pub(crate) fn normalize_optional_filter(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

/// 验证会话引用（SessionRef）的有效性。
///
/// # 参数
/// - `value` - 会话引用
/// - `field` - 字段名称
///
/// # Errors
/// 如果 `serving_node_id` 小于等于 0 或 `session_id` 为空，返回 `Error::Validation`。
pub fn validate_session_ref(value: &SessionRef, field: &str) -> Result<()> {
    validate_positive_i64(value.serving_node_id, &format!("{field}.serving_node_id"))?;
    if value.session_id.trim().is_empty() {
        return Err(Error::validation(format!("{field}.session_id is required")));
    }
    Ok(())
}

/// 验证投递模式的合法性。
///
/// `DeliveryMode::Unspecified` 被视为无效，必须显式指定投递模式。
///
/// # 参数
/// - `mode` - 投递模式
///
/// # Errors
/// 如果投递模式为 `Unspecified`，返回 `Error::Validation`。
pub fn validate_delivery_mode(mode: DeliveryMode) -> Result<()> {
    match mode {
        DeliveryMode::BestEffort | DeliveryMode::RouteRetry => Ok(()),
        DeliveryMode::Unspecified => Err(Error::validation("invalid delivery_mode \"\"")),
    }
}

/// 验证用户元数据键的格式。
///
/// 键不能为空，长度不能超过 128 字符，且只能包含字母、数字、点号、下划线、冒号和连字符。
///
/// # 参数
/// - `value` - 元数据键
/// - `field` - 字段名称
///
/// # Errors
/// 如果键为空、过长或包含不支持的字符，返回 `Error::Validation`。
pub fn validate_user_metadata_key(value: &str, field: &str) -> Result<()> {
    validate_user_metadata_key_fragment(value, field, false)
}

/// 验证可选的用户元数据键片段。
///
/// 与 `validate_user_metadata_key` 类似，但允许空值（用于可选的键前缀和游标参数）。
///
/// # 参数
/// - `value` - 可选的元数据键片段
/// - `field` - 字段名称
///
/// # Errors
/// 如果非空值过长或包含不支持的字符，返回 `Error::Validation`。
pub fn validate_optional_user_metadata_key_fragment(value: &str, field: &str) -> Result<()> {
    validate_user_metadata_key_fragment(value, field, true)
}

/// 验证用户元数据扫描的 limit 参数。
///
/// limit 必须是非负数且不能超过最大限制（1000）。
///
/// # 参数
/// - `limit` - 扫描限制数
///
/// # Errors
/// 如果 limit 为负数或超过最大值，返回 `Error::Validation`。
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
