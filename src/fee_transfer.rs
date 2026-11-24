//! Epoch-aware Token-2022 transfers with fees.

use crate::types::{CtResult, SigResult};

use solana_client::rpc_client::RpcClient;

use solana_sdk::{
    pubkey::Pubkey,
    signature::Signer,
    transaction::Transaction,
};

use spl_token_2022::{
    extension::{
        transfer_fee::{
            instruction::transfer_checked_with_fee,
            TransferFeeConfig,
        },
        BaseStateWithExtensions,
        StateWithExtensions,
    },
    state::Mint,
};

/// Read the current epoch and calculate the applicable fee.
pub fn calculate_current_fee(
    client: &RpcClient,
    mint: &Pubkey,
    amount: u64,
) -> CtResult<u64> {
    let account = client.get_account(mint)?;

    if account.owner != spl_token_2022::id() {
        return Err("Mint is not owned by Token-2022".into());
    }

    let mint_state =
        StateWithExtensions::<Mint>::unpack(
            &account.data,
        )?;

    let fee_config =
        mint_state.get_extension::<TransferFeeConfig>()?;

    let epoch = client.get_epoch_info()?.epoch;

    let fee = fee_config
        .calculate_epoch_fee(epoch, amount)
        .ok_or("Unable to calculate transfer fee")?;

    Ok(fee)
}

/// Transfer tokens using the current epoch fee.
pub fn transfer_with_fee(
    client: &RpcClient,
    payer: &dyn Signer,
    authority: &dyn Signer,
    source: &Pubkey,
    mint: &Pubkey,
    destination: &Pubkey,
    amount: u64,
) -> SigResult {
    let mint_account = client.get_account(mint)?;

    if mint_account.owner != spl_token_2022::id() {
        return Err("Mint is not owned by Token-2022".into());
    }

    let mint_state =
        StateWithExtensions::<Mint>::unpack(
            &mint_account.data,
        )?;

    let decimals = mint_state.base.decimals;

    let fee = calculate_current_fee(
        client,
        mint,
        amount,
    )?;

    let ix = transfer_checked_with_fee(
        &spl_token_2022::id(),
        source,
        mint,
        destination,
        &authority.pubkey(),
        &[],
        amount,
        decimals,
        fee,
    )?;

    let blockhash = client.get_latest_blockhash()?;

    let transaction = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[payer, authority],
        blockhash,
    );

    let signature =
        client.send_and_confirm_transaction(&transaction)?;

    println!("Transfer successful");
    println!("Amount: {} base units", amount);
    println!("Fee: {} base units", fee);
    println!("Net amount: {} base units", amount - fee);
    println!("Transaction: {}", signature);

    Ok(signature)
}
