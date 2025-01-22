use crate::kamino::utils::errors::LendingError;

pub type LendingResult<T = ()> = std::result::Result<T, LendingError>;