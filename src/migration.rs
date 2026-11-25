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

/// Migrate public V1 tokens into V2 tokens atomically.
///
/// The owner authorizes the burn and the administrator
/// authorizes the replacement minting.
pub fn migrate_v1_to_v2(
    client: &RpcClient,
    payer: &dyn Signer,
    owner: &dyn Signer,
    administrator: &dyn Signer,
    v1_mint: &solana_sdk::pubkey::Pubkey,
    v2_mint: &solana_sdk::pubkey::Pubkey,
    amount: u64,
) -> crate::types::SigResult {
    use spl_associated_token_account::
        get_associated_token_address_with_program_id;

    use spl_token_2022::{
        extension::{
            BaseStateWithExtensions,
            StateWithExtensions,
        },
        instruction::{burn_checked, mint_to_checked},
        state::Account,
    };

    if amount == 0 {
        return Err("Migration amount must be positive".into());
    }

    if v1_mint == v2_mint {
        return Err("V1 and V2 mints must be different".into());
    }

    let program_id = spl_token_2022::id();
    let admin = administrator.pubkey();
    let owner_address = owner.pubkey();

    // Read both mint accounts.
    let v1_data = client.get_account(v1_mint)?;
    let v2_data = client.get_account(v2_mint)?;

    if v1_data.owner != program_id
        || v2_data.owner != program_id
    {
        return Err("Invalid mint program owner".into());
    }

    let v1_state =
        StateWithExtensions::<Mint>::unpack(
            &v1_data.data,
        )?;

    let v2_state =
        StateWithExtensions::<Mint>::unpack(
            &v2_data.data,
        )?;

    // Verify matching decimals.
    if v1_state.base.decimals != v2_state.base.decimals {
        return Err("Mint decimals do not match".into());
    }

    let decimals = v1_state.base.decimals;

    // Verify administrator authority.
    if v1_state.base.mint_authority != Some(admin).into()
        || v2_state.base.mint_authority != Some(admin).into()
    {
        return Err("Unauthorized migration administrator".into());
    }

    // Verify expected V1 extensions.
    let expected_v1 = [
        ExtensionType::TransferFeeConfig,
        ExtensionType::MetadataPointer,
        ExtensionType::DefaultAccountState,
        ExtensionType::MintCloseAuthority,
    ];

    let v1_extensions = v1_state.get_extension_types()?;

    if v1_extensions.len() != expected_v1.len()
        || !expected_v1.iter().all(
            |extension| v1_extensions.contains(extension)
        )
    {
        return Err("Invalid V1 mint extensions".into());
    }

    // Verify expected V2 extensions.
    let v2_extensions = v2_state.get_extension_types()?;

    if v2_extensions.len() != V2_EXTENSIONS.len()
        || !V2_EXTENSIONS.iter().all(
            |extension| v2_extensions.contains(extension)
        )
    {
        return Err("Invalid V2 mint extensions".into());
    }

    // Derive the owner's token accounts.
    let source =
        get_associated_token_address_with_program_id(
            &owner_address,
            v1_mint,
            &program_id,
        );

    let destination =
        get_associated_token_address_with_program_id(
            &owner_address,
            v2_mint,
            &program_id,
        );

    let source_data = client.get_account(&source)?;
    let destination_data = client.get_account(&destination)?;

    if source_data.owner != program_id
        || destination_data.owner != program_id
    {
        return Err("Invalid token account program owner".into());
    }

    let source_state =
        StateWithExtensions::<Account>::unpack(
            &source_data.data,
        )?;

    let destination_state =
        StateWithExtensions::<Account>::unpack(
            &destination_data.data,
        )?;

    // Verify account ownership and mint association.
    if source_state.base.owner != owner_address
        || destination_state.base.owner != owner_address
    {
        return Err("Token account owner mismatch".into());
    }

    if source_state.base.mint != *v1_mint
        || destination_state.base.mint != *v2_mint
    {
        return Err("Token account mint mismatch".into());
    }

    // Both accounts must be thawed.
    if source_state.base.state != AccountState::Initialized
        || destination_state.base.state != AccountState::Initialized
    {
        return Err("Both accounts require KYC approval".into());
    }

    if source_state.base.amount < amount {
        return Err("Insufficient V1 token balance".into());
    }

    // Instruction 1: burn original V1 tokens.
    let burn_ix = burn_checked(
        &program_id,
        &source,
        v1_mint,
        &owner_address,
        &[],
        amount,
        decimals,
    )?;

    // Instruction 2: mint replacement V2 tokens.
    let mint_ix = mint_to_checked(
        &program_id,
        v2_mint,
        &destination,
        &admin,
        &[],
        amount,
        decimals,
    )?;

    // Both instructions execute atomically.
    let blockhash = client.get_latest_blockhash()?;

    let transaction = Transaction::new_signed_with_payer(
        &[burn_ix, mint_ix],
        Some(&payer.pubkey()),
        &[payer, owner, administrator],
        blockhash,
    );

    let signature =
        client.send_and_confirm_transaction(
            &transaction,
        )?;

    println!("Atomic V1 to V2 migration successful");
    println!("V1 mint: {}", v1_mint);
    println!("V2 mint: {}", v2_mint);
    println!("Burned V1 base units: {}", amount);
    println!("Minted V2 base units: {}", amount);
    println!("Transaction: {}", signature);

    Ok(signature)
}
