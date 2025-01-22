use anchor_lang::prelude::*;

pub type MarginfiResult<G = ()> = Result<G>;

pub use crate::{
    marginfi::utils::errors::MarginfiError,
    marginfi::models::group::{GroupConfig, MarginfiGroup},
};
