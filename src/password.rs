use bcrypt::{hash, DEFAULT_COST};

use crate::errors::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasswordSource {
    Plain,
    Hashed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PasswordInput {
    source: PasswordSource,
    encoded: String,
}

impl PasswordInput {
    pub fn validate(&self) -> Result<()> {
        if self.encoded.is_empty() {
            return Err(Error::validation("password is required"));
        }
        Ok(())
    }

    pub fn wire_value(&self) -> Result<&str> {
        self.validate()?;
        Ok(self.encoded.as_str())
    }

    pub fn source(&self) -> PasswordSource {
        self.source
    }

    pub fn encoded(&self) -> &str {
        &self.encoded
    }

    pub fn is_hashed(&self) -> bool {
        !self.encoded.is_empty()
    }

    pub fn is_zero(&self) -> bool {
        self.encoded.is_empty()
    }
}

pub fn hash_password(plain: impl AsRef<str>) -> Result<String> {
    let plain = plain.as_ref();
    if plain.is_empty() {
        return Err(Error::validation("password is required"));
    }
    hash(plain, DEFAULT_COST).map_err(|err| Error::connection("bcrypt", err.to_string()))
}

pub fn plain_password(plain: impl AsRef<str>) -> Result<PasswordInput> {
    Ok(PasswordInput {
        source: PasswordSource::Plain,
        encoded: hash_password(plain)?,
    })
}

pub fn hashed_password(value: impl Into<String>) -> PasswordInput {
    PasswordInput {
        source: PasswordSource::Hashed,
        encoded: value.into(),
    }
}
