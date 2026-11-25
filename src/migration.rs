//! Regulated confidential Token-2022 mint reissuance.

use crate::types::CtResult;

use crate::extensions::{
    DECIMALS,
    FEE_BASIS_POINTS,
    MAXIMUM_FEE,
};

use solana_client::rpc_client::RpcClient;

use solana_sdk::{
    signature::{Keypair, Signer},
    transaction::Transaction,
};

use solana_system_interface::instruction as system_instruction;

use spl_token_2022::{
    extension::{
        confidential_transfer::instruction::{
            initialize_mint as initialize_confidential_mint,
        },
        confidential_transfer_fee::instruction::{
            initialize_confidential_transfer_fee_config,
        },
        default_account_state::instruction::{
            initialize_default_account_state,
        },
        metadata_pointer::instruction::{
            initialize as initialize_metadata_pointer,
        },
        transfer_fee::instruction::{
            initialize_transfer_fee_config,
        },
        ExtensionType,
    },
    instruction::{
        initialize_mint2,
        initialize_mint_close_authority,
        initialize_permanent_delegate,
    },
    solana_zk_sdk::encryption::pod::elgamal::PodElGamalPubkey,
    state::{AccountState, Mint},
};

pub const V2_EXTENSIONS: [ExtensionType; 7] = [
    ExtensionType::TransferFeeConfig,
    ExtensionType::MetadataPointer,
    ExtensionType::DefaultAccountState,
    ExtensionType::MintCloseAuthority,
    ExtensionType::PermanentDelegate,
    ExtensionType::ConfidentialTransferMint,
    ExtensionType::ConfidentialTransferFeeConfig,
];

/// Calculate and validate V2 mint account size.
pub fn regulated_confidential_mint_size() -> CtResult<usize> {
    ExtensionType::check_for_invalid_mint_extension_combinations(
        &V2_EXTENSIONS,
    )?;

    Ok(
        ExtensionType::try_calculate_account_len::<Mint>(
            &V2_EXTENSIONS,
        )?
    )
}

/// Create the replacement regulated confidential mint.
pub fn create_regulated_confidential_mint(
    client: &RpcClient,
    payer: &dyn Signer,
    administrator: &dyn Signer,
) -> CtResult<Keypair> {
    let mint = Keypair::new();
    let mint_address = mint.pubkey();
    let admin = administrator.pubkey();
    let program_id = spl_token_2022::id();

    let space = regulated_confidential_mint_size()?;

    let rent =
        client.get_minimum_balance_for_rent_exemption(
            space,
        )?;

    // Derive the administrator's confidential keys.
    let (elgamal_keypair, _) =
        crate::keys::derive_confidential_keys(
            administrator,
        )?;

    let pubkey_bytes = elgamal_keypair.pubkey().to_bytes();

    let fee_elgamal_pubkey: PodElGamalPubkey =
        bytemuck::pod_read_unaligned(&pubkey_bytes);

    // Create the mint account.
    let create_ix = system_instruction::create_account(
        &payer.pubkey(),
        &mint_address,
        rent,
        space as u64,
        &program_id,
    );

    // 1. Public transfer fees.
    let fee_ix = initialize_transfer_fee_config(
        &program_id,
        &mint_address,
        Some(&admin),
        Some(&admin),
        FEE_BASIS_POINTS,
        MAXIMUM_FEE,
    )?;

    // 2. Self-referencing metadata pointer.
    let metadata_ix = initialize_metadata_pointer(
        &program_id,
        &mint_address,
        Some(admin),
        Some(mint_address),
    )?;

    // 3. Default frozen token accounts.
    let default_state_ix =
        initialize_default_account_state(
            &program_id,
            &mint_address,
            &AccountState::Frozen,
        )?;

    // 4. Mint close authority.
    let close_authority_ix =
        initialize_mint_close_authority(
            &program_id,
            &mint_address,
            Some(&admin),
        )?;

    // 5. Permanent Delegate.
    let delegate_ix = initialize_permanent_delegate(
        &program_id,
        &mint_address,
        &admin,
    )?;

    // 6. Confidential transfers with manual approval.
    let confidential_ix = initialize_confidential_mint(
        &program_id,
        &mint_address,
        Some(admin),
        false,
        None,
    )?;

    // 7. Confidential transfer fees.
    let confidential_fee_ix =
        initialize_confidential_transfer_fee_config(
            &program_id,
            &mint_address,
            Some(admin),
            &fee_elgamal_pubkey,
        )?;

    // Initialize the base mint last.
    let initialize_mint_ix = initialize_mint2(
        &program_id,
        &mint_address,
        &admin,
        Some(&admin),
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
            delegate_ix,
            confidential_ix,
            confidential_fee_ix,
            initialize_mint_ix,
        ],
        Some(&payer.pubkey()),
        &[payer, &mint],
        blockhash,
    );

    let signature =
        client.send_and_confirm_transaction(
            &transaction,
        )?;

    println!("V2 regulated confidential mint created");
    println!("Mint: {}", mint_address);
    println!("Mint size: {} bytes", space);
    println!("Permanent Delegate: {}", admin);
    println!("Confidential approval: Manual");
    println!("Default account state: Frozen");
    println!("Transaction: {}", signature);

    Ok(mint)
}
