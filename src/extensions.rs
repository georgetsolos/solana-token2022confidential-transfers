//! Token-2022 regulated mint extensions.

use crate::types::CtResult;

use solana_client::rpc_client::RpcClient;

use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
};

use solana_system_interface::instruction as system_instruction;

use spl_token_2022::{
    extension::{
        default_account_state::instruction::
            initialize_default_account_state,
        metadata_pointer::instruction::
            initialize as initialize_metadata_pointer,
        transfer_fee::instruction::
            initialize_transfer_fee_config,
        ExtensionType,
    },
    instruction::{
        initialize_mint2,
        initialize_mint_close_authority,
    },
    state::{AccountState, Mint},
};

pub const DECIMALS: u8 = 9;
pub const FEE_BASIS_POINTS: u16 = 100;
pub const MAXIMUM_FEE: u64 = 100_000_000;

/// Calculate the required mint account size.
pub fn regulated_mint_size() -> CtResult<usize> {
    let extensions = [
        ExtensionType::TransferFeeConfig,
        ExtensionType::MetadataPointer,
        ExtensionType::DefaultAccountState,
        ExtensionType::MintCloseAuthority,
    ];

    Ok(
        ExtensionType::try_calculate_account_len::<Mint>(
            &extensions,
        )?
    )
}

/// Create a regulated Token-2022 mint.
pub fn create_regulated_mint(
    client: &RpcClient,
    payer: &dyn Signer,
    administrator: &Pubkey,
) -> CtResult<Keypair> {
    let mint = Keypair::new();
    let mint_address = mint.pubkey();
    let program_id = spl_token_2022::id();

    let space = regulated_mint_size()?;

    let rent =
        client.get_minimum_balance_for_rent_exemption(
            space,
        )?;

    let create_ix = system_instruction::create_account(
        &payer.pubkey(),
        &mint_address,
        rent,
        space as u64,
        &program_id,
    );

    // Extension 1: Transfer fees.
    let fee_ix = initialize_transfer_fee_config(
        &program_id,
        &mint_address,
        Some(administrator),
        Some(administrator),
        FEE_BASIS_POINTS,
        MAXIMUM_FEE,
    )?;

    // Extension 2: Self-referencing metadata pointer.
    let metadata_ix = initialize_metadata_pointer(
        &program_id,
        &mint_address,
        Some(*administrator),
        Some(mint_address),
    )?;

    // Extension 3: Accounts start frozen.
    let default_state_ix =
        initialize_default_account_state(
            &program_id,
            &mint_address,
            &AccountState::Frozen,
        )?;

    // Extension 4: Mint close authority.
    let close_authority_ix =
        initialize_mint_close_authority(
            &program_id,
            &mint_address,
            Some(administrator),
        )?;

    // Initialize the base mint AFTER all extensions.
    let initialize_mint_ix = initialize_mint2(
        &program_id,
        &mint_address,
        administrator,
        Some(administrator),
        DECIMALS,
    )?;

    let blockhash = client.get_latest_blockhash()?;

    let transaction = Transaction::new_signed_with_payer(
        &[
            create_ix,
            fee_ix,
            metadata_ix,
            default_state_ix,
            close_authority_ix,
            initialize_mint_ix,
        ],
        Some(&payer.pubkey()),
        &[payer, &mint],
        blockhash,
    );

    let signature =
        client.send_and_confirm_transaction(&transaction)?;

    println!("Regulated mint created: {}", mint_address);
    println!("Mint account size: {} bytes", space);
    println!("Transfer fee: {} basis points", FEE_BASIS_POINTS);
    println!("Default account state: Frozen");
    println!("Transaction: {}", signature);

    Ok(mint)
}
