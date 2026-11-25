//! Manual approval of confidential token accounts.

use crate::types::{CtResult, SigResult};

use solana_client::rpc_client::RpcClient;

use solana_sdk::{
    pubkey::Pubkey,
    signature::Signer,
    transaction::Transaction,
};

use spl_token_2022::{
    extension::{
        confidential_transfer::{
            instruction::approve_account,
            ConfidentialTransferAccount,
        },
        BaseStateWithExtensions,
        StateWithExtensions,
    },
    state::Account,
};

/// Approve a configured confidential token account.
pub fn approve_confidential_account(
    client: &RpcClient,
    payer: &dyn Signer,
    administrator: &dyn Signer,
    mint: &Pubkey,
    token_account: &Pubkey,
) -> SigResult {
    let ix = approve_account(
        &spl_token_2022::id(),
        token_account,
        mint,
        &administrator.pubkey(),
        &[],
    )?;

    let blockhash = client.get_latest_blockhash()?;

    let transaction = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[payer, administrator],
        blockhash,
    );

    let signature =
        client.send_and_confirm_transaction(
            &transaction,
        )?;

    println!("Confidential account approved");
    println!("Account: {}", token_account);
    println!("Transaction: {}", signature);

    Ok(signature)
}

/// Read the confidential account approval status.
pub fn is_confidential_account_approved(
    client: &RpcClient,
    token_account: &Pubkey,
) -> CtResult<bool> {
    let account = client.get_account(token_account)?;

    if account.owner != spl_token_2022::id() {
        return Err(
            "Account is not owned by Token-2022".into()
        );
    }

    let state =
        StateWithExtensions::<Account>::unpack(
            &account.data,
        )?;

    let confidential_state =
        state.get_extension::<ConfidentialTransferAccount>()?;

    Ok(bool::from(confidential_state.approved))
}
