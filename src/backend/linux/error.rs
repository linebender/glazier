use std::fmt;

#[derive(Debug)]
pub enum Error {
    #[cfg(feature = "wayland")]
    Wayland(crate::backend::wayland::error::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, _f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match *self {
            #[cfg(feature = "wayland")]
            Error::Wayland(ref it) => write!(_f, "{}", it),
        }
    }
}
#[cfg(feature = "wayland")]
impl From<crate::backend::wayland::error::Error> for Error {
    fn from(value: crate::backend::wayland::error::Error) -> Self {
        Self::Wayland(value)
    }
}
