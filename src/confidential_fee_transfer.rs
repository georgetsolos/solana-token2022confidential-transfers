//! Confidential Token-2022 transfers with transfer fees.

use crate::types::*;
use crate::transfer::{
    available_balance,
    send_tx,
    stage_range_proof_record,
    cast_elgamal_pubkey_legacy_to_v6,
    cast_elgamal_ciphertext_legacy_to_v6,
    cast_elgamal_ciphertext_v6_to_legacy,
    cast_ae_ciphertext_legacy_to_v6,
    cast_ae_ciphertext_v6_to_legacy,
};

use solana_address::Address;
use solana_client::rpc_client::RpcClient;

use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signature, Signer},
};

use solana_system_interface::instruction as system_instruction;

use solana_zk_elgamal_proof_interface::{
    instruction::{
        close_context_state,
        ContextStateInfo,
        ProofInstruction,
    },
    proof_data::{
        CiphertextCommitmentEqualityProofContext,
        BatchedGroupedCiphertext3HandlesValidityProofContext,
        PercentageWithCapProofContext,
        BatchedGroupedCiphertext2HandlesValidityProofContext,
        BatchedRangeProofContext,
    },
    state::ProofContextState,
};

use solana_zk_sdk::encryption::elgamal::{
    ElGamalCiphertext,
    ElGamalPubkey,
};

use spl_associated_token_account::
    get_associated_token_address_with_program_id;

use spl_token_2022::{
    extension::{
        confidential_transfer::{
            instruction::transfer_with_fee,
            ConfidentialTransferAccount,
            ConfidentialTransferMint,
        },
        confidential_transfer_fee::ConfidentialTransferFeeConfig,
        transfer_fee::TransferFeeConfig,
        BaseStateWithExtensions,
        StateWithExtensions,
    },
    solana_zk_sdk::encryption::pod::elgamal::
        PodElGamalPubkey as PodElGamalPubkeyLegacy,
    state::{Account as TokenAccount, Mint},
};

use spl_token_confidential_transfer_proof_extraction::
    instruction::ProofLocation;

use spl_token_confidential_transfer_proof_generation::
    transfer_with_fee::transfer_with_fee_split_proof_data;

use std::mem::size_of;

const ZK_PROOF_PROGRAM_ID: Pubkey =
    solana_sdk::pubkey!(
        "ZkE1Gama1Proof11111111111111111111111111111"
    );

const RECORD_PROOF_OFFSET: u32 = 33;

/// Stage and verify a proof using a temporary SPL Record account.
fn create_proof_context(
    client: &RpcClient,
    payer: &dyn Signer,
    authority: &Pubkey,
    proof_instruction: ProofInstruction,
    proof_bytes: &[u8],
    context_size: usize,
    signatures: &mut Vec<Signature>,
) -> CtResult<Keypair> {
    let record = Keypair::new();
    let context = Keypair::new();

    // Stage the proof separately to avoid transaction-size overflow.
    let mut staged = stage_range_proof_record(
        client,
        payer,
        &record,
        proof_bytes,
        &[],
        &[],
    )?;

    signatures.append(&mut staged);

    let rent =
        client.get_minimum_balance_for_rent_exemption(
            context_size,
        )?;

    let create_ix = system_instruction::create_account(
        &payer.pubkey(),
        &context.pubkey(),
        rent,
        context_size as u64,
        &ZK_PROOF_PROGRAM_ID,
    );

    let context_address =
        Address::from(context.pubkey().to_bytes());

    let authority_address =
        Address::from(authority.to_bytes());

    let record_address =
        Address::from(record.pubkey().to_bytes());

    let verify_ix =
        proof_instruction.encode_verify_proof_from_account(
            Some(ContextStateInfo {
                context_state_account: &context_address,
                context_state_authority: &authority_address,
            }),
            &record_address,
            RECORD_PROOF_OFFSET,
        );

    let close_record_ix =
        spl_record::instruction::close_account(
            &record.pubkey(),
            &payer.pubkey(),
            &payer.pubkey(),
        );

    let signature = send_tx(
        client,
        &[create_ix, verify_ix, close_record_ix],
        &[payer, &context],
        &payer.pubkey(),
    )?;

    signatures.push(signature);

    println!(
        "Proof context verified: {}",
        context.pubkey()
    );

    Ok(context)
}

/// Transfer confidential tokens using the current epoch fee.
pub async fn transfer_confidential_with_fee(
    client: &RpcClient,
    payer: &dyn Signer,
    sender: &Keypair,
    mint: &Pubkey,
    recipient: &Pubkey,
    amount: u64,
) -> MultiSigResult {
    if amount == 0 {
        return Err("Transfer amount must be positive".into());
    }

    let program_id = spl_token_2022::id();

    let sender_account =
        get_associated_token_address_with_program_id(
            &sender.pubkey(),
            mint,
            &program_id,
        );

    let recipient_account =
        get_associated_token_address_with_program_id(
            recipient,
            mint,
            &program_id,
        );

    if sender_account == recipient_account {
        return Err("Self-transfer is not supported".into());
    }

    // Read mint extensions and current fee parameters.
    let mint_data = client.get_account(mint)?;

    if mint_data.owner != program_id {
        return Err("Invalid mint program owner".into());
    }

    let mint_state =
        StateWithExtensions::<Mint>::unpack(
            &mint_data.data,
        )?;

    let fee_config =
        mint_state.get_extension::<TransferFeeConfig>()?;

    let confidential_mint =
        mint_state.get_extension::<ConfidentialTransferMint>()?;

    let confidential_fee =
        mint_state.get_extension::<ConfidentialTransferFeeConfig>()?;

    let epoch = client.get_epoch_info()?.epoch;
    let epoch_fee = fee_config.get_epoch_fee(epoch);

    let fee_basis_points: u16 =
        epoch_fee.transfer_fee_basis_points.into();

    let maximum_fee: u64 =
        epoch_fee.maximum_fee.into();

    let fee = fee_config
        .calculate_epoch_fee(epoch, amount)
        .ok_or("Unable to calculate transfer fee")?;

    let net_amount = amount
        .checked_sub(fee)
        .ok_or("Invalid transfer fee")?;

    println!("Current epoch: {}", epoch);
    println!("Fee basis points: {}", fee_basis_points);
    println!("Maximum fee: {}", maximum_fee);
    println!("Gross amount: {}", amount);
    println!("Expected fee: {}", fee);
    println!("Expected net amount: {}", net_amount);

    // Read fee authority encryption key.
    let fee_authority_pubkey: ElGamalPubkey =
        cast_elgamal_pubkey_legacy_to_v6(
            &confidential_fee
                .withdraw_withheld_authority_elgamal_pubkey,
        )?
        .try_into()
        .map_err(|e| format!("Fee authority key: {e:?}"))?;

    // Read optional auditor key.
    let auditor_pod: Option<PodElGamalPubkeyLegacy> =
        confidential_mint.auditor_elgamal_pubkey.into();

    let auditor_pubkey: Option<ElGamalPubkey> =
        auditor_pod
            .map(|pod| -> CtResult<ElGamalPubkey> {
                cast_elgamal_pubkey_legacy_to_v6(&pod)?
                    .try_into()
                    .map_err(|e| {
                        format!("Auditor key: {e:?}").into()
                    })
            })
            .transpose()?;

    // Read recipient confidential account.
    let recipient_data =
        client.get_account(&recipient_account)?;

    let recipient_state =
        StateWithExtensions::<TokenAccount>::unpack(
            &recipient_data.data,
        )?;

    let recipient_ext =
        recipient_state
            .get_extension::<ConfidentialTransferAccount>()?;

    if !bool::from(recipient_ext.approved) {
        return Err(
            "Recipient confidential account is not approved".into()
        );
    }

    let recipient_pubkey: ElGamalPubkey =
        cast_elgamal_pubkey_legacy_to_v6(
            &recipient_ext.elgamal_pubkey,
        )?
        .try_into()
        .map_err(|e| format!("Recipient key: {e:?}"))?;

    // Read sender confidential account.
    let sender_data =
        client.get_account(&sender_account)?;

    let sender_state =
        StateWithExtensions::<TokenAccount>::unpack(
            &sender_data.data,
        )?;

    let sender_ext =
        sender_state
            .get_extension::<ConfidentialTransferAccount>()?;

    if !bool::from(sender_ext.approved) {
        return Err(
            "Sender confidential account is not approved".into()
        );
    }

    let (sender_elgamal, sender_aes) =
        crate::keys::derive_confidential_keys(sender)?;

    let current_available: ElGamalCiphertext =
        cast_elgamal_ciphertext_legacy_to_v6(
            &sender_ext.available_balance,
        )?
        .try_into()
        .map_err(|e| format!("Available balance: {e:?}"))?;

    let cached_balance =
        cast_ae_ciphertext_legacy_to_v6(
            &sender_ext.decryptable_available_balance,
        )?;

    let available_amount = available_balance(
        &current_available,
        &cached_balance,
        &sender_elgamal,
        &sender_aes,
    )?;

    let remaining_amount = available_amount
        .checked_sub(amount)
        .ok_or("Insufficient confidential balance")?;

    let current_decryptable =
        sender_aes.encrypt(available_amount);

    // Generate all five cryptographic proofs.
    println!("Generating five confidential fee proofs");

    let proofs = transfer_with_fee_split_proof_data(
        &current_available,
        &current_decryptable,
        amount,
        &sender_elgamal,
        &sender_aes,
        &recipient_pubkey,
        auditor_pubkey.as_ref(),
        &fee_authority_pubkey,
        fee_basis_points,
        maximum_fee,
    )
    .map_err(|e| format!("Fee proof generation: {e:?}"))?;

    let mut signatures = Vec::new();

    // Proof 1: ciphertext-commitment equality.
    let equality = create_proof_context(
        client,
        payer,
        &sender.pubkey(),
        ProofInstruction::VerifyCiphertextCommitmentEquality,
        bytemuck::bytes_of(&proofs.equality_proof_data),
        size_of::<ProofContextState<
            CiphertextCommitmentEqualityProofContext
        >>(),
        &mut signatures,
    )?;

    // Proof 2: transfer amount ciphertext validity.
    let validity = create_proof_context(
        client,
        payer,
        &sender.pubkey(),
        ProofInstruction::VerifyBatchedGroupedCiphertext3HandlesValidity,
        bytemuck::bytes_of(
            &proofs
                .transfer_amount_ciphertext_validity_proof_data_with_ciphertext
                .proof_data,
        ),
        size_of::<ProofContextState<
            BatchedGroupedCiphertext3HandlesValidityProofContext
        >>(),
        &mut signatures,
    )?;

    // Proof 3: percentage with maximum fee cap.
    let percentage = create_proof_context(
        client,
        payer,
        &sender.pubkey(),
        ProofInstruction::VerifyPercentageWithCap,
        bytemuck::bytes_of(
            &proofs.percentage_with_cap_proof_data,
        ),
        size_of::<ProofContextState<
            PercentageWithCapProofContext
        >>(),
        &mut signatures,
    )?;

    // Proof 4: fee ciphertext validity.
    let fee_validity = create_proof_context(
        client,
        payer,
        &sender.pubkey(),
        ProofInstruction::VerifyBatchedGroupedCiphertext2HandlesValidity,
        bytemuck::bytes_of(
            &proofs.fee_ciphertext_validity_proof_data,
        ),
        size_of::<ProofContextState<
            BatchedGroupedCiphertext2HandlesValidityProofContext
        >>(),
        &mut signatures,
    )?;

    // Proof 5: U256 range proof.
    let range = create_proof_context(
        client,
        payer,
        &sender.pubkey(),
        ProofInstruction::VerifyBatchedRangeProofU256,
        bytemuck::bytes_of(&proofs.range_proof_data),
        size_of::<ProofContextState<
            BatchedRangeProofContext
        >>(),
        &mut signatures,
    )?;

    let new_decryptable =
        cast_ae_ciphertext_v6_to_legacy(
            &sender_aes.encrypt(remaining_amount),
        );

    let auditor_lo =
        cast_elgamal_ciphertext_v6_to_legacy(
            &proofs
                .transfer_amount_ciphertext_validity_proof_data_with_ciphertext
                .ciphertext_lo,
        );

    let auditor_hi =
        cast_elgamal_ciphertext_v6_to_legacy(
            &proofs
                .transfer_amount_ciphertext_validity_proof_data_with_ciphertext
                .ciphertext_hi,
        );

    let equality_key = equality.pubkey();
    let validity_key = validity.pubkey();
    let percentage_key = percentage.pubkey();
    let fee_validity_key = fee_validity.pubkey();
    let range_key = range.pubkey();

    let instructions = transfer_with_fee(
        &program_id,
        &sender_account,
        mint,
        &recipient_account,
        &new_decryptable,
        &auditor_lo,
        &auditor_hi,
        &sender.pubkey(),
        &[],
        ProofLocation::ContextStateAccount(&equality_key),
        ProofLocation::ContextStateAccount(&validity_key),
        ProofLocation::ContextStateAccount(&percentage_key),
        ProofLocation::ContextStateAccount(&fee_validity_key),
        ProofLocation::ContextStateAccount(&range_key),
    )?;

    let payer_address =
        Address::from(payer.pubkey().to_bytes());

    let sender_address =
        Address::from(sender.pubkey().to_bytes());

    // Execute transfer and close all proof contexts atomically.
    let mut instructions = instructions;

    for key in [
        equality_key,
        validity_key,
        percentage_key,
        fee_validity_key,
        range_key,
    ] {
        let context_address =
            Address::from(key.to_bytes());

        instructions.push(
            close_context_state(
                ContextStateInfo {
                    context_state_account: &context_address,
                    context_state_authority: &sender_address,
                },
                &payer_address,
            )
        );
    }

    let transfer_signature = send_tx(
        client,
        &instructions,
        &[payer, sender],
        &payer.pubkey(),
    )?;

    signatures.push(transfer_signature);

    println!("Confidential transfer with fee successful");
    println!("Gross amount: {}", amount);
    println!("Expected withheld fee: {}", fee);
    println!("Expected recipient credit: {}", net_amount);
    println!("Expected sender remaining: {}", remaining_amount);
    println!("Transfer signature: {}", transfer_signature);
    println!("Total transactions: {}", signatures.len());

    Ok(signatures)
}
