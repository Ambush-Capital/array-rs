use std::str::FromStr;

use crate::{
    bank_authority_seed, bank_seed,
    marginfi::models::{
        account::MarginfiAccount,
        group::{Bank, BankVaultType},
    },
    marginfi::utils::constants::{ASSET_TAG_DEFAULT, ASSET_TAG_SOL, ASSET_TAG_STAKED},
    marginfi::utils::errors::MarginfiError,
    marginfi::utils::prelude::MarginfiResult,
};
use anchor_lang::prelude::*;
use fixed::types::I80F48;

pub fn find_bank_vault_pda(bank_pk: &Pubkey, vault_type: BankVaultType) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        bank_seed!(vault_type, bank_pk),
        &Pubkey::from_str("MFv2hWf31Z9kbCa1snEPYctwafyhdvnV7FZnsebVacA").unwrap(),
    )
}

pub fn find_bank_vault_authority_pda(bank_pk: &Pubkey, vault_type: BankVaultType) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        bank_authority_seed!(vault_type, bank_pk),
        &Pubkey::from_str("MFv2hWf31Z9kbCa1snEPYctwafyhdvnV7FZnsebVacA").unwrap(),
    )
}

pub trait NumTraitsWithTolerance<T> {
    fn is_zero_with_tolerance(&self, t: T) -> bool;
    fn is_positive_with_tolerance(&self, t: T) -> bool;
}

impl<T> NumTraitsWithTolerance<T> for I80F48
where
    I80F48: PartialOrd<T>,
{
    fn is_zero_with_tolerance(&self, t: T) -> bool {
        self.abs() < t
    }

    fn is_positive_with_tolerance(&self, t: T) -> bool {
        self.gt(&t)
    }
}

/// A minimal tool to convert a hex string like "22f123639" into the byte equivalent.
pub fn hex_to_bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks(2)
        .map(|chunk| {
            let high = chunk[0] as char;
            let low = chunk[1] as char;
            let high = high.to_digit(16).expect("Invalid hex character") as u8;
            let low = low.to_digit(16).expect("Invalid hex character") as u8;
            (high << 4) | low
        })
        .collect()
}

/// Validate that after a deposit to Bank, the users's account contains either all Default/SOL
/// balances, or all Staked/Sol balances. Default and Staked assets cannot mix.
pub fn validate_asset_tags(bank: &Bank, marginfi_account: &MarginfiAccount) -> MarginfiResult {
    let mut has_default_asset = false;
    let mut has_staked_asset = false;

    for balance in marginfi_account.lending_account.balances.iter() {
        if balance.active {
            match balance.bank_asset_tag {
                ASSET_TAG_DEFAULT => has_default_asset = true,
                ASSET_TAG_SOL => { /* Do nothing, SOL can mix with any asset type */ }
                ASSET_TAG_STAKED => has_staked_asset = true,
                _ => panic!("unsupported asset tag"),
            }
        }
    }

    // 1. Regular assets (DEFAULT) cannot mix with Staked assets
    if bank.config.asset_tag == ASSET_TAG_DEFAULT && has_staked_asset {
        return err!(MarginfiError::AssetTagMismatch);
    }

    // 2. Staked SOL cannot mix with Regular asset (DEFAULT)
    if bank.config.asset_tag == ASSET_TAG_STAKED && has_default_asset {
        return err!(MarginfiError::AssetTagMismatch);
    }

    Ok(())
}

/// Validate that two banks are compatible based on their asset tags. See the following combinations
/// (* is wildcard, e.g. any tag):
///
/// Allowed:
/// 1) Default/Default
/// 2) Sol/*
/// 3) Staked/Staked
///
/// Forbidden:
/// 1) Default/Staked
///
/// Returns an error if the two banks have mismatching asset tags according to the above.
pub fn validate_bank_asset_tags(bank_a: &Bank, bank_b: &Bank) -> MarginfiResult {
    let is_bank_a_default = bank_a.config.asset_tag == ASSET_TAG_DEFAULT;
    let is_bank_a_staked = bank_a.config.asset_tag == ASSET_TAG_STAKED;
    let is_bank_b_default = bank_b.config.asset_tag == ASSET_TAG_DEFAULT;
    let is_bank_b_staked = bank_b.config.asset_tag == ASSET_TAG_STAKED;
    // Note: Sol is compatible with all other tags and doesn't matter...

    // 1. Default assets cannot mix with Staked assets
    if is_bank_a_default && is_bank_b_staked {
        return err!(MarginfiError::AssetTagMismatch);
    }
    if is_bank_a_staked && is_bank_b_default {
        return err!(MarginfiError::AssetTagMismatch);
    }

    Ok(())
}
