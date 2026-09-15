//! Migration bridge to the kernal-api-owned error facade.

pub(crate) use kernal_api::error::{message, Context, Error, Result};

macro_rules! bail {
    ($($argument:tt)*) => {
        return Err($crate::error_compat::message(format_args!($($argument)*)))
    };
}

pub(crate) use bail;

macro_rules! error {
    ($($argument:tt)*) => {
        $crate::error_compat::message(format_args!($($argument)*))
    };
}

pub(crate) use error;
