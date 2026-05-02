use bcrypt::{hash, DEFAULT_COST};

use crate::errors::{Error, Result};

/// 密码来源类型。
///
/// 指示密码输入的来源类型：
/// - `Plain` - 原始明文密码，SDK 会自动进行 bcrypt 哈希
/// - `Hashed` - 已经过 bcrypt 哈希的密码
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasswordSource {
    /// 明文密码，SDK 将自动对其进行 bcrypt 哈希处理
    Plain,
    /// 已经过 bcrypt 哈希的密码，SDK 将直接使用
    Hashed,
}

/// 密码输入类型，封装了密码的来源和编码值。
///
/// 通过 `plain_password()` 或 `hashed_password()` 工厂函数创建。
///
/// # 使用场景
/// - 用户注册/登录时提供明文密码（使用 `plain_password()`）
/// - 从配置文件或数据库中读取已哈希的密码（使用 `hashed_password()`）
///
/// # 示例
///
/// ```ignore
/// use turntf::password::{plain_password, hashed_password};
///
/// let plain = plain_password("my_secret_password")?;
/// let hashed = hashed_password("$2b$12$LJ3m4ys3Lg3YOCwNkTkYJO...");
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PasswordInput {
    source: PasswordSource,
    encoded: String,
}

impl PasswordInput {
    /// 验证密码不为空。
    ///
    /// # Errors
    /// 如果密码编码值为空，返回 `Error::Validation` 错误。
    pub fn validate(&self) -> Result<()> {
        if self.encoded.is_empty() {
            return Err(Error::validation("password is required"));
        }
        Ok(())
    }

    /// 获取用于网络传输的密码值（经过 bcrypt 哈希的字符串）。
    ///
    /// 在调用此方法时会自动执行 `validate()` 检查。
    ///
    /// # Errors
    /// 如果密码为空，返回验证错误。
    pub fn wire_value(&self) -> Result<&str> {
        self.validate()?;
        Ok(self.encoded.as_str())
    }

    /// 获取密码的来源类型。
    pub fn source(&self) -> PasswordSource {
        self.source
    }

    /// 获取密码的编码值（bcrypt 哈希值）。
    pub fn encoded(&self) -> &str {
        &self.encoded
    }

    /// 检查密码是否已设置（非空）。
    ///
    /// 返回 `true` 表示密码已设置，`false` 表示密码为空。
    #[allow(unused)]
    pub fn is_hashed(&self) -> bool {
        !self.encoded.is_empty()
    }

    /// 检查密码是否为空。
    ///
    /// 返回 `true` 表示密码未被设置。
    pub fn is_zero(&self) -> bool {
        self.encoded.is_empty()
    }
}

/// 对明文密码进行 bcrypt 哈希处理。
///
/// 使用默认的 bcrypt 成本系数对密码进行哈希。
///
/// # 参数
/// - `plain` - 明文密码字符串
///
/// # Errors
/// 如果密码为空，返回 `Error::Validation` 错误。
/// 如果 bcrypt 哈希失败，返回 `Error::Connection` 错误。
///
/// # 示例
///
/// ```ignore
/// let hashed = hash_password("my_secret_password")?;
/// ```
pub fn hash_password(plain: impl AsRef<str>) -> Result<String> {
    let plain = plain.as_ref();
    if plain.is_empty() {
        return Err(Error::validation("password is required"));
    }
    hash(plain, DEFAULT_COST).map_err(|err| Error::connection("bcrypt", err.to_string()))
}

/// 使用明文密码创建 `PasswordInput`。
///
/// SDK 会自动对明文密码进行 bcrypt 哈希处理，然后封装为 `PasswordInput`。
///
/// # 参数
/// - `plain` - 明文密码字符串
///
/// # Errors
/// 如果密码为空或 bcrypt 哈希失败，返回相应的错误。
///
/// # 示例
///
/// ```ignore
/// let password = plain_password("my_secret_password")?;
/// ```
pub fn plain_password(plain: impl AsRef<str>) -> Result<PasswordInput> {
    Ok(PasswordInput {
        source: PasswordSource::Plain,
        encoded: hash_password(plain)?,
    })
}

/// 使用已哈希的密码创建 `PasswordInput`。
///
/// 当密码已经过 bcrypt 哈希处理时（例如从数据库读取），使用此函数创建密码输入。
///
/// # 参数
/// - `value` - 已经过 bcrypt 哈希的密码字符串
///
/// # 示例
///
/// ```ignore
/// let password = hashed_password("$2b$12$LJ3m4ys3Lg3YOCwNkTkYJO...");
/// ```
pub fn hashed_password(value: impl Into<String>) -> PasswordInput {
    PasswordInput {
        source: PasswordSource::Hashed,
        encoded: value.into(),
    }
}
