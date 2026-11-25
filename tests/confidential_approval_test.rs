//! Manual confidential approval integration test.

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
    confidential_approval::{
        approve_confidential_account,
        is_confidential_account_approved,
    },
    deposit::deposit_to_confidential,
    apply_pending::apply_pending_balance,
    extensions::DECIMALS,
};

use solana_sdk::signature::{Keypair, Signer};

use spl_token_2022::{
    extension::{
        confidential_transfer_fee::ConfidentialTransferFeeAmount,
        BaseStateWithExtensions,
        StateWithExtensions,
    },
    state::Account,
};

#[tokio::test(flavor = "multi_thread")]
async fn test_manual_confidential_approval() {
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
    let unauthorized_user = Keypair::new();

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

    // Mint one public token.
    mint_tokens(
        &env,
        &mint_address,
        &token_account,
        &administrator,
        1_000_000_000,
    ).unwrap();

    // Owner configures confidential transfers.
    configure_account_for_confidential_transfers(
        &env.client,
        &env.payer,
        &owner,
        &mint_address,
    ).await.expect(
        "V2 confidential configuration failed"
    );

    println!("PASS: V2 confidential account configured");

    // Verify confidential fee extension.
    let account_data = env.client
        .get_account(&token_account)
        .unwrap();

    let account_state =
        StateWithExtensions::<Account>::unpack(
            &account_data.data,
        ).unwrap();

    assert!(
        account_state
            .get_extension::<ConfidentialTransferFeeAmount>()
            .is_ok()
    );

    println!("PASS: Confidential fee extension initialized");

    // Manual approval must initially be false.
    assert!(
        !is_confidential_account_approved(
            &env.client,
            &token_account,
        ).unwrap()
    );

    println!("PASS: Account requires manual approval");

    // Deposit before approval must fail.
    let blocked_deposit = deposit_to_confidential(
        &env.client,
        &env.payer,
        &owner,
        &mint_address,
        500_000_000,
        DECIMALS,
    ).await;

    assert!(
        blocked_deposit.is_err(),
        "Deposit before approval should fail"
    );

    println!("PASS: Deposit before approval rejected");

    // Unauthorized approval must fail.
    let unauthorized_approval =
        approve_confidential_account(
            &env.client,
            &env.payer,
            &unauthorized_user,
            &mint_address,
            &token_account,
        );

    assert!(
        unauthorized_approval.is_err(),
        "Unauthorized approval should fail"
    );

    assert!(
        !is_confidential_account_approved(
            &env.client,
            &token_account,
        ).unwrap()
    );

    println!("PASS: Unauthorized approval rejected");

    // Administrator approves the account.
    approve_confidential_account(
        &env.client,
        &env.payer,
        &administrator,
        &mint_address,
        &token_account,
    ).unwrap();

    assert!(
        is_confidential_account_approved(
            &env.client,
            &token_account,
        ).unwrap()
    );

    println!("PASS: Administrator approval successful");

    // Deposit 0.5 tokens after approval.
    deposit_to_confidential(
        &env.client,
        &env.payer,
        &owner,
        &mint_address,
        500_000_000,
        DECIMALS,
    ).await.unwrap();

    println!("PASS: Confidential deposit successful");

    // Apply pending balance.
    apply_pending_balance(
        &env.client,
        &env.payer,
        &owner,
        &mint_address,
    ).await.unwrap();

    println!("PASS: Pending balance applied");

    // Verify remaining public balance.
    let final_account = env.client
        .get_account(&token_account)
        .unwrap();

    let final_state =
        StateWithExtensions::<Account>::unpack(
            &final_account.data,
        ).unwrap();

    assert_eq!(
        final_state.base.amount,
        500_000_000
    );

    println!("PASS: Remaining public balance is 0.5 tokens");

    println!("====================================");
    println!("MANUAL CONFIDENTIAL APPROVAL PASSED");
    println!("====================================");
}
