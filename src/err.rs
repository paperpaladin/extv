use std::fmt;

// Two kinds, because they mean different things to the caller and get
// different exit statuses. Malformed is "the image says something
// impossible and extv will not guess"; Io is "the file could not be read",
// which is the operator's problem, not the filesystem's.
#[derive(Debug)]
pub enum Error {
    Malformed(String),
    Io(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Malformed(m) => write!(f, "malformed image: {}", m),
            Error::Io(m) => write!(f, "{}", m),
        }
    }
}

impl Error {
    pub fn exit_code(&self) -> u8 {
        match self {
            Error::Malformed(_) => 2,
            Error::Io(_) => 3,
        }
    }
}

pub fn bad<T>(m: impl Into<String>) -> Result<T> {
    Err(Error::Malformed(m.into()))
}
