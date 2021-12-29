use std::{
    error::Error,
    fmt::{Debug, Display},
};

pub struct ErrorWrapper<T> {
    wrapped: T,
}

impl<T> Debug for ErrorWrapper<T>
where
    T: Debug,
{
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(fmt, "ErrorWrapper {{wrapped: {:?}}}", self.wrapped)
    }
}

impl<T> Display for ErrorWrapper<T>
where
    T: Display,
{
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(fmt, "Wrapped Error: {}", self.wrapped)
    }
}
impl<T> Error for ErrorWrapper<T> where T: Display + Debug {}

pub trait WrappableError {
    type Wrapped;
    fn wrap_error(self) -> Self::Wrapped;
}

impl WrappableError for String {
    type Wrapped = ErrorWrapper<String>;
    fn wrap_error(self) -> Self::Wrapped {
        ErrorWrapper { wrapped: self }
    }
}

impl<U, T> WrappableError for Result<U, T> {
    type Wrapped = Result<U, ErrorWrapper<T>>;
    fn wrap_error(self) -> Self::Wrapped {
        self.map_err(|e| ErrorWrapper { wrapped: e })
    }
}
