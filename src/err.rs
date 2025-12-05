use std::fmt;

// Two kinds, because they mean different things to the caller and get
// different exit statuses. Malformed is "the image says something
// impossible and extv will not guess"; Io is "the file could not be read",
// which is the operator's problem, not the filesystem's.
#[derive(Debug)]
pub enum Error {
    Malformed(String),
    Io(String),
    // The reader closed the pipe, which is what `extv groups img | head`
    // does every time. Rust ignores SIGPIPE and turns the failed write
    // into a panic, and a panic exits 101, which is the status `make
    // overread` uses to mean the sanity check fired. Leaving it would
    // have made that gate unable to tell a planted over-read from a
    // closed pipe.
    Pipe,
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Malformed(m) => write!(f, "malformed image: {}", m),
            Error::Io(m) => write!(f, "{}", m),
            Error::Pipe => write!(f, "output pipe closed"),
        }
    }
}

impl Error {
    pub fn exit_code(&self) -> u8 {
        match self {
            Error::Malformed(_) => 2,
            Error::Io(_) => 3,
            Error::Pipe => 0,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            Error::Pipe
        } else {
            Error::Io(e.to_string())
        }
    }
}

pub fn bad<T>(m: impl Into<String>) -> Result<T> {
    Err(Error::Malformed(m.into()))
}
