use crate::adapters::common::{before_check, invoke_process};
use crate::error::ErrorCode;
use crate::{obric_v2_program, HopAccounts, SWAP_SELECTOR};
use anchor_lang::{prelude::*, solana_program::instruction::Instruction};
use anchor_spl::token::Token;
use anchor_spl::token_interface::{Mint, TokenAccount};
use arrayref::array_ref;

use super::common::DexProcessor;

const ARGS_LEN: usize = 25;

pub struct ObricV2Processor;
impl DexProcessor for ObricV2Processor {}

//this dex only supoort spltoken not support token_2022
pub struct ObricV2Account<'info> {
    pub dex_program_id: &'info AccountInfo<'info>,
    pub swap_authority_pubkey: &'info AccountInfo<'info>,
    pub swap_source_token: InterfaceAccount<'info, TokenAccount>,
    pub swap_destination_token: InterfaceAccount<'info, TokenAccount>,

    pub trading_pair: &'info AccountInfo<'info>,
    pub token_mint_x: InterfaceAccount<'info, Mint>,
    pub token_mint_y: InterfaceAccount<'info, Mint>,
    pub reserve_x: InterfaceAccount<'info, TokenAccount>,
    pub reserve_y: InterfaceAccount<'info, TokenAccount>,
    pub protocol_fee: &'info AccountInfo<'info>,
    pub x_price_feed: &'info AccountInfo<'info>,
    pub y_price_feed: &'info AccountInfo<'info>,
    pub token_program: Program<'info, Token>,
}
pub const ACCOUNTS_LEN: usize = 13;

impl<'info> ObricV2Account<'info> {
    fn parse_accounts(accounts: &'info [AccountInfo<'info>], offset: usize) -> Result<Self> {
        let [
            dex_program_id,
            swap_authority_pubkey,
            swap_source_token,
            swap_destination_token,
            trading_pair,
            token_mint_x,
            token_mint_y,
            reserve_x,
            reserve_y,
            protocol_fee,
            x_price_feed,
            y_price_feed,
            token_program,
        ]: & [AccountInfo<'info>; ACCOUNTS_LEN] = array_ref![accounts, offset, ACCOUNTS_LEN];
        Ok(Self {
            dex_program_id: dex_program_id,
            swap_authority_pubkey: swap_authority_pubkey,
            swap_source_token: InterfaceAccount::try_from(swap_source_token)?,
            swap_destination_token: InterfaceAccount::try_from(swap_destination_token)?,
            trading_pair: trading_pair,
            token_mint_x: InterfaceAccount::try_from(token_mint_x)?,
            token_mint_y: InterfaceAccount::try_from(token_mint_y)?,
            reserve_x: InterfaceAccount::try_from(reserve_x)?,
            reserve_y: InterfaceAccount::try_from(reserve_y)?,
            protocol_fee: protocol_fee,
            x_price_feed: x_price_feed,
            y_price_feed: y_price_feed,
            token_program: Program::try_from(token_program)?,
        })
    }
}

pub fn swap<'a>(
    remaining_accounts: &'a [AccountInfo<'a>],
    amount_in: u64,
    offset: &mut usize,
    hop_accounts: &mut HopAccounts,
    hop: usize,
    proxy_swap: bool,
) -> Result<u64> {
    msg!("Dex::Obric v2 amount_in: {}, offset: {}", amount_in, offset);
    require!(
        remaining_accounts.len() >= *offset + ACCOUNTS_LEN,
        ErrorCode::InvalidAccountsLength
    );
    let mut swap_accounts = ObricV2Account::parse_accounts(remaining_accounts, *offset)?;
    if swap_accounts.dex_program_id.key != &obric_v2_program::id() {
        return Err(ErrorCode::InvalidProgramId.into());
    }

    // check hop accounts & swap authority
    let swap_source_token = swap_accounts.swap_source_token.key();
    let swap_destination_token = swap_accounts.swap_destination_token.key();
    before_check(
        &swap_accounts.swap_authority_pubkey,
        swap_source_token,
        swap_destination_token,
        hop_accounts,
        hop,
        proxy_swap,
    )?;

    let x_to_y: bool;
    // compare mint pubkey
    if swap_accounts.swap_destination_token.mint == swap_accounts.token_mint_x.key() {
        x_to_y = false;
    } else if swap_accounts.swap_destination_token.mint == swap_accounts.token_mint_y.key() {
        x_to_y = true;
    } else {
        return Err(ErrorCode::InvalidTokenMint.into());
    }

    let (user_token_account_x, user_token_account_y) = if swap_accounts.swap_source_token.mint
        == swap_accounts.token_mint_x.key()
        && swap_accounts.swap_destination_token.mint == swap_accounts.token_mint_y.key()
    {
        (
            swap_accounts.swap_source_token.key(),
            swap_accounts.swap_destination_token.key(),
        )
    } else if swap_accounts.swap_source_token.mint == swap_accounts.token_mint_y.key()
        && swap_accounts.swap_destination_token.mint == swap_accounts.token_mint_x.key()
    {
        (
            swap_accounts.swap_destination_token.key(),
            swap_accounts.swap_source_token.key(),
        )
    } else {
        return Err(ErrorCode::InvalidTokenMint.into());
    };

    let mut data = Vec::with_capacity(ARGS_LEN);
    data.extend_from_slice(SWAP_SELECTOR);
    data.extend_from_slice(&(x_to_y as u8).to_le_bytes());
    data.extend_from_slice(&amount_in.to_le_bytes());
    data.extend_from_slice(&1u64.to_le_bytes());

    let accounts = vec![
        AccountMeta::new(swap_accounts.trading_pair.key(), false),
        AccountMeta::new_readonly(swap_accounts.token_mint_x.key(), false),
        AccountMeta::new_readonly(swap_accounts.token_mint_y.key(), false),
        AccountMeta::new(swap_accounts.reserve_x.key(), false),
        AccountMeta::new(swap_accounts.reserve_y.key(), false),
        AccountMeta::new(user_token_account_x.key(), false),
        AccountMeta::new(user_token_account_y.key(), false),
        AccountMeta::new(swap_accounts.protocol_fee.key(), false),
        AccountMeta::new_readonly(swap_accounts.x_price_feed.key(), false),
        AccountMeta::new_readonly(swap_accounts.y_price_feed.key(), false),
        AccountMeta::new_readonly(swap_accounts.swap_authority_pubkey.key(), true),
        AccountMeta::new_readonly(swap_accounts.token_program.key(), false),
    ];

    let account_infos = vec![
        swap_accounts.trading_pair.to_account_info(),
        swap_accounts.token_mint_x.to_account_info(),
        swap_accounts.token_mint_y.to_account_info(),
        swap_accounts.reserve_x.to_account_info(),
        swap_accounts.reserve_y.to_account_info(),
        swap_accounts.swap_source_token.to_account_info(),
        swap_accounts.swap_destination_token.to_account_info(),
        swap_accounts.protocol_fee.to_account_info(),
        swap_accounts.x_price_feed.to_account_info(),
        swap_accounts.y_price_feed.to_account_info(),
        swap_accounts.swap_authority_pubkey.to_account_info(),
        swap_accounts.token_program.to_account_info(),
    ];

    let instruction = Instruction {
        program_id: swap_accounts.dex_program_id.key(),
        accounts,
        data,
    };

    let dex_processor = &ObricV2Processor;
    let amount_out = invoke_process(
        dex_processor,
        &account_infos,
        swap_source_token,
        &mut swap_accounts.swap_destination_token,
        hop_accounts,
        instruction,
        hop,
        offset,
        ACCOUNTS_LEN,
        proxy_swap,
    )?;
    Ok(amount_out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn test_pack_swap_instruction() {
        let amount_in = 100u64;
        let x_to_y = true;
        let mut data = Vec::with_capacity(ARGS_LEN);
        data.extend_from_slice(SWAP_SELECTOR);
        data.extend_from_slice(&(x_to_y as u8).to_le_bytes());
        data.extend_from_slice(&amount_in.to_le_bytes());
        data.extend_from_slice(&1u64.to_le_bytes());

        msg!("data.len: {}", data.len());
        assert!(data.len() == ARGS_LEN);
    }
}
