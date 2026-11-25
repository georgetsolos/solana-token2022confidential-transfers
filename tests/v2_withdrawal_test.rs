mod common;

use common::{
    TestEnv,
    create_token_account,
    mint_tokens,
};

use conf_balances_examples::{
    migration::create_regulated_confidential_mint,
    kyc::approve_kyc,
    configure::configure_account_for_confidential_transfers,
    confidential_approval::approve_confidential_account,
    deposit::deposit_to_confidential,
    apply_pending::apply_pending_balance,
    withdraw::withdraw_from_confidential,
    keys::derive_confidential_keys,
    extensions::DECIMALS,
};

use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};

use solana_zk_sdk::encryption::elgamal::ElGamalCiphertext;

use solana_zk_sdk_pod::encryption::elgamal::
    PodElGamalCiphertext;

use spl_token_2022::{
    extension::{
        confidential_transfer::ConfidentialTransferAccount,
        confidential_transfer_fee::ConfidentialTransferFeeAmount,
        BaseStateWithExtensions,
        StateWithExtensions,
    },
    state::Account,
};

/// Independently read public and confidential balances.
fn read_balances(
    env: &TestEnv,
    token_account: &Pubkey,
    owner: &Keypair,
) -> (u64, u64) {
    let data = env.client
        .get_account(token_account)
        .unwrap();

    let state =
        StateWithExtensions::<Account>::unpack(
            &data.data,
        ).unwrap();

    let extension = state
        .get_extension::<ConfidentialTransferAccount>()
        .unwrap();

    let (keys, _) =
        derive_confidential_keys(owner).unwrap();

    let ciphertext: ElGamalCiphertext =
        PodElGamalCiphertext(
            bytemuck::bytes_of(
                &extension.available_balance,
            ).try_into().unwrap()
        ).try_into().unwrap();

    let confidential = ciphertext
        .decrypt_u32(keys.secret())
        .expect("Unable to decrypt confidential balance");

    (state.base.amount, confidential as u64)
}

#[tokio::test(flavor = "multi_thread")]
async fn test_v2_confidential_withdrawal() {
    let env = TestEnv::new();

    assert!(
        env.is_local,
        "This test must run on Localnet"
    );

    env.airdrop_if_needed(
        &env.payer_pubkey(),
        10_000_000_000,
    ).unwrap();

    let administrator = Keypair::new();
    let owner = Keypair::new();

    const MINT_AMOUNT: u64 = 1_000_000_000;
    const DEPOSIT_AMOUNT: u64 = 800_000_000;
    const WITHDRAW_AMOUNT: u64 = 100_000_000;

    println!("====================================");
    println!("V2 CONFIDENTIAL WITHDRAWAL TEST");
    println!("====================================");

    // Create the regulated V2 mint.
    let mint = create_regulated_confidential_mint(
        &env.client,
        &env.payer,
        &administrator,
    ).unwrap();

    let mint_address = mint.pubkey();

    // Create the owner's token account.
    let token_account = create_token_account(
        &env,
        &mint_address,
        &owner.pubkey(),
    ).unwrap();

    // Approve KYC.
    approve_kyc(
        &env.client,
        &env.payer,
        &administrator,
        &mint_address,
        &token_account,
    ).unwrap();

    println!("PASS: KYC approved");

    // Configure confidential transfers.
    configure_account_for_confidential_transfers(
        &env.client,
        &env.payer,
        &owner,
        &mint_address,
    ).await.unwrap();

    // Administrator manually approves the account.
    approve_confidential_account(
        &env.client,
        &env.payer,
        &administrator,
        &mint_address,
        &token_account,
    ).unwrap();

    println!("PASS: Confidential account approved");

    // Mint one token.
    mint_tokens(
        &env,
        &mint_address,
        &token_account,
        &administrator,
        MINT_AMOUNT,
    ).unwrap();

    // Deposit 0.8 tokens.
    deposit_to_confidential(
        &env.client,
        &env.payer,
        &owner,
        &mint_address,
        DEPOSIT_AMOUNT,
        DECIMALS,
    ).await.unwrap();

    // Apply pending balance before withdrawal.
    apply_pending_balance(
        &env.client,
        &env.payer,
        &owner,
        &mint_address,
    ).await.unwrap();

    println!("PASS: Pending balance applied");

    // Verify balances before withdrawal.
    let before = read_balances(
        &env,
        &token_account,
        &owner,
    );

    assert_eq!(
        before,
        (200_000_000, 800_000_000)
    );

    println!("PASS: Initial public balance = 0.200");
    println!("PASS: Initial confidential balance = 0.800");

    // Reject a withdrawal exceeding available balance.
    let excessive_withdrawal = withdraw_from_confidential(
        &env.client,
        &env.payer,
        &owner,
        &mint_address,
        900_000_000,
        DECIMALS,
    ).await;

    let error = excessive_withdrawal
        .expect_err("Excessive withdrawal must fail");

    assert!(
        error.to_string()
            .contains("Insufficient confidential balance"),
        "Unexpected error: {}",
        error
    );

    assert_eq!(
        read_balances(&env, &token_account, &owner),
        before
    );

    println!("PASS: Excessive withdrawal rejected");
    println!("PASS: Balances unchanged after rejection");

    // Withdraw 0.1 tokens to public balance.
    let signature = withdraw_from_confidential(
        &env.client,
        &env.payer,
        &owner,
        &mint_address,
        WITHDRAW_AMOUNT,
        DECIMALS,
    ).await.unwrap();

    println!("Withdrawal transaction: {}", signature);

    // Independently verify final balances.
    let after = read_balances(
        &env,
        &token_account,
        &owner,
    );

    assert_eq!(
        after,
        (300_000_000, 700_000_000)
    );

    println!("PASS: Final public balance = 0.300");
    println!("PASS: Final confidential balance = 0.700");

    // Verify token conservation.
    assert_eq!(
        after.0 + after.1,
        MINT_AMOUNT
    );

    println!("PASS: Total token balance conserved");

    // Verify no encrypted fee was withheld.
    let account_data = env.client
        .get_account(&token_account)
        .unwrap();

    let state =
        StateWithExtensions::<Account>::unpack(
            &account_data.data,
        ).unwrap();

    let fee_extension = state
        .get_extension::<ConfidentialTransferFeeAmount>()
        .unwrap();

    let (admin_keys, _) =
        derive_confidential_keys(&administrator)
            .unwrap();

    let fee_ciphertext: ElGamalCiphertext =
        PodElGamalCiphertext(
            bytemuck::bytes_of(
                &fee_extension.withheld_amount,
            ).try_into().unwrap()
        ).try_into().unwrap();

    let withheld_fee = fee_ciphertext
        .decrypt_u32(admin_keys.secret())
        .expect("Unable to decrypt withheld fee");

    assert_eq!(withheld_fee, 0);

    println!("PASS: No confidential withdrawal fee");

    println!("====================================");
    println!("V2 CONFIDENTIAL WITHDRAWAL PASSED");
    println!("====================================");
}
