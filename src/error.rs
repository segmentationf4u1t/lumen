use std::fmt;

#[derive(Debug, Clone)]
pub struct Error(pub String);

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    pub fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }

    pub fn last_os(what: &str) -> Self {
        let code = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        let io = std::io::Error::from_raw_os_error(code as i32);
        Self(format!("{what}: {io} (win32 {code})"))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self(e.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self(format!("json: {e}"))
    }
}

impl From<String> for Error {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for Error {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}
