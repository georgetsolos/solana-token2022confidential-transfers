//! KYC account approval and freeze management.

use crate::types::{CtResult, SigResult};

use solana_client::rpc_client::RpcClient;

use solana_sdk::{
    pubkey::Pubkey,
    signature::Signer,
    transaction::Transaction,
};

use spl_token_2022::{
    extension::StateWithExtensions,
    instruction::{freeze_account, thaw_account},
    state::{Account, AccountState},
};

/// Approve a token account after KYC verification.
pub fn approve_kyc(
    client: &RpcClient,
    payer: &dyn Signer,
    freeze_authority: &dyn Signer,
    mint: &Pubkey,
    token_account: &Pubkey,
) -> SigResult {
    let ix = thaw_account(
        &spl_token_2022::id(),
        token_account,
        mint,
        &freeze_authority.pubkey(),
        &[],
    )?;

    let blockhash = client.get_latest_blockhash()?;

    let transaction = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[payer, freeze_authority],
        blockhash,
    );

    let signature =
        client.send_and_confirm_transaction(&transaction)?;

    println!("KYC approved: {}", token_account);
    println!("Account thawed successfully");
    println!("Transaction: {}", signature);

    Ok(signature)
}

/// Revoke approval by freezing a token account.
pub fn revoke_kyc(
    client: &RpcClient,
    payer: &dyn Signer,
    freeze_authority: &dyn Signer,
    mint: &Pubkey,
    token_account: &Pubkey,
) -> SigResult {
    let ix = freeze_account(
        &spl_token_2022::id(),
        token_account,
        mint,
        &freeze_authority.pubkey(),
        &[],
    )?;

    let blockhash = client.get_latest_blockhash()?;

    let transaction = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[payer, freeze_authority],
        blockhash,
    );

    let signature =
        client.send_and_confirm_transaction(&transaction)?;

    println!("KYC approval revoked: {}", token_account);
    println!("Account frozen successfully");
    println!("Transaction: {}", signature);

    Ok(signature)
}

/// Read the current Token-2022 account state.
pub fn get_account_state(
    client: &RpcClient,
    token_account: &Pubkey,
) -> CtResult<AccountState> {
    let account = client.get_account(token_account)?;

    if account.owner != spl_token_2022::id() {
        return Err("Account is not owned by Token-2022".into());
    }

    let state =
        StateWithExtensions::<Account>::unpack(
            &account.data,
        )?;

    Ok(state.base.state)
}

/// Check whether the account is currently frozen.
pub fn is_account_frozen(
    client: &RpcClient,
    token_account: &Pubkey,
) -> CtResult<bool> {
    Ok(
        get_account_state(client, token_account)?
            == AccountState::Frozen
    )
}
