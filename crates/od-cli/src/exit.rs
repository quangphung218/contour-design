//! An error that ends `od` with a specific exit code (spec 2.6: 66 when a
//! plugin lacks capabilities).

use std::fmt;

#[derive(Debug)]
pub struct Exit {
    pub code: u8,
    pub message: String,
    /// Already printed (e.g. a JSON error on stderr): `main` stays quiet.
    pub printed: bool,
}

impl Exit {
    pub fn new(code: u8, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            printed: false,
        }
    }

    pub fn printed(code: u8, message: impl Into<String>) -> Self {
        Self {
            printed: true,
            ..Self::new(code, message)
        }
    }
}

impl fmt::Display for Exit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Exit {}
