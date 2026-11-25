mod common;

use common::{
    TestEnv,
    create_token_account,
    mint_tokens,
};

use conf_balances_examples::{
    migration::create_regulated_confidential_mint,
    kyc::{
        approve_kyc,
        revoke_kyc,
        is_account_frozen,
    },
    configure::configure_account_for_confidential_transfers,
    confidential_approval::approve_confidential_account,
    deposit::deposit_to_confidential,
    apply_pending::apply_pending_balance,
    withdraw::withdraw_from_confidential,
    fee_transfer::transfer_with_fee,
    keys::derive_confidential_keys,
    extensions::DECIMALS,
};

use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};

use solana_zk_sdk::encryption::elgamal::ElGamalCiphertext;
use solana_zk_sdk_pod::encryption::elgamal::PodElGamalCiphertext;

use spl_token_2022::{
    extension::{
        confidential_transfer::ConfidentialTransferAccount,
        BaseStateWithExtensions,
        StateWithExtensions,
    },
    state::Account,
};

fn read_balances(
    env: &TestEnv,
    address: &Pubkey,
    owner: &Keypair,
) -> (u64, u64) {
    let data = env.client
        .get_account(address)
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
        .expect("Unable to decrypt balance");

    (state.base.amount, confidential as u64)
}

#[tokio::test(flavor = "multi_thread")]
async fn test_permanent_delegate_security_boundary() {
    let env = TestEnv::new();

    assert!(
        env.is_local,
        "Security test must run on Localnet"
    );

    env.airdrop_if_needed(
        &env.payer_pubkey(),
        10_000_000_000,
    ).unwrap();

    let administrator = Keypair::new();
    let owner = Keypair::new();

    println!("====================================");
    println!("PERMANENT DELEGATE SECURITY TEST");
    println!("====================================");

    let mint = create_regulated_confidential_mint(
        &env.client,
        &env.payer,
        &administrator,
    ).unwrap();

    let mint_address = mint.pubkey();

    let source = create_token_account(
        &env,
        &mint_address,
        &owner.pubkey(),
    ).unwrap();

    let treasury = create_token_account(
        &env,
        &mint_address,
        &administrator.pubkey(),
    ).unwrap();

    approve_kyc(
        &env.client,
        &env.payer,
        &administrator,
        &mint_address,
        &source,
    ).unwrap();

    approve_kyc(
        &env.client,
        &env.payer,
        &administrator,
        &mint_address,
        &treasury,
    ).unwrap();

    configure_account_for_confidential_transfers(
        &env.client,
        &env.payer,
        &owner,
        &mint_address,
    ).await.unwrap();

    approve_confidential_account(
        &env.client,
        &env.payer,
        &administrator,
        &mint_address,
        &source,
    ).unwrap();

    mint_tokens(
        &env,
        &mint_address,
        &source,
        &administrator,
        1_000_000_000,
    ).unwrap();

    // Establish a synthetic mixed-balance account.
    deposit_to_confidential(
        &env.client,
        &env.payer,
        &owner,
        &mint_address,
        800_000_000,
        DECIMALS,
    ).await.unwrap();

    apply_pending_balance(
        &env.client,
        &env.payer,
        &owner,
        &mint_address,
    ).await.unwrap();

    assert_eq!(
        read_balances(&env, &source, &owner),
        (200_000_000, 800_000_000)
    );

    println!("PASS: Initial balances verified");

    // Verify administrative authority over public tokens.
    transfer_with_fee(
        &env.client,
        &env.payer,
        &administrator,
        &source,
        &mint_address,
        &treasury,
        200_000_000,
    ).unwrap();

    assert_eq!(
        read_balances(&env, &source, &owner),
        (0, 800_000_000)
    );

    println!("PASS: Permanent Delegate public transfer");

    // Verify treasury credit after the 1% transfer fee.
    let treasury_data = env.client
        .get_account(&treasury)
        .unwrap();

    let treasury_state =
        StateWithExtensions::<Account>::unpack(
            &treasury_data.data,
        ).unwrap();

    assert_eq!(
        treasury_state.base.amount,
        198_000_000
    );

    println!("PASS: Treasury received expected net amount");

    // Verify that ordinary transfers cannot spend
    // the confidential balance.
    let result = transfer_with_fee(
        &env.client,
        &env.payer,
        &administrator,
        &source,
        &mint_address,
        &treasury,
        100_000_000,
    );

    let error = result.expect_err(
        "Ordinary transfer must not spend confidential funds"
    );

    assert!(
        error.to_string().contains("custom program error: 0x1;"),
        "Unexpected transfer rejection: {}",
        error
    );

    assert!(
        error.to_string().contains("custom program error: 0x1;"),
        "Unexpected transfer rejection: {}",
        error
    );

    println!("Transfer rejection: {}", error);

    assert_eq!(
        read_balances(&env, &source, &owner),
        (0, 800_000_000)
    );

    println!("PASS: Confidential balance remained intact");

    // Apply the administrative containment control.
    revoke_kyc(
        &env.client,
        &env.payer,
        &administrator,
        &mint_address,
        &source,
    ).unwrap();

    assert!(
        is_account_frozen(
            &env.client,
            &source,
        ).unwrap()
    );

    println!("PASS: Account frozen by administrator");

    // Verify withdrawal is blocked while frozen.
    let result = withdraw_from_confidential(
        &env.client,
        &env.payer,
        &owner,
        &mint_address,
        100_000_000,
        DECIMALS,
    ).await;

    let error = result.expect_err(
        "Frozen account withdrawal must fail"
    );

    assert!(
        error.to_string().contains("custom program error: 0x11;"),
        "Unexpected withdrawal rejection: {}",
        error
    );

    assert!(
        error.to_string().contains("custom program error: 0x11;"),
        "Unexpected withdrawal rejection: {}",
        error
    );

    println!("Withdrawal rejection: {}", error);

    assert_eq!(
        read_balances(&env, &source, &owner),
        (0, 800_000_000)
    );

    println!("PASS: Frozen withdrawal rejected");
    println!("PASS: Confidential balance preserved");

    println!("====================================");
    println!("PERMANENT DELEGATE SECURITY TEST PASSED");
    println!("====================================");
}
