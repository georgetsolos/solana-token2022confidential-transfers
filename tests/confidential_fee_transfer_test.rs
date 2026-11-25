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
    confidential_fee_transfer::transfer_confidential_with_fee,
    keys::derive_confidential_keys,
    extensions::DECIMALS,
};

use solana_sdk::signature::{Keypair, Signer};

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

#[tokio::test(flavor = "multi_thread")]
async fn test_confidential_transfer_with_fee() {
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
    let sender = Keypair::new();
    let recipient = Keypair::new();

    const MINT_AMOUNT: u64 = 1_000_000_000;
    const DEPOSIT_AMOUNT: u64 = 800_000_000;
    const TRANSFER_AMOUNT: u64 = 200_000_000;
    const EXPECTED_FEE: u64 = 2_000_000;
    const EXPECTED_NET: u64 = 198_000_000;
    const EXPECTED_SENDER: u64 = 600_000_000;

    println!("====================================");
    println!("CONFIDENTIAL TRANSFER WITH FEE TEST");
    println!("====================================");

    // Create V2 regulated confidential mint.
    let mint = create_regulated_confidential_mint(
        &env.client,
        &env.payer,
        &administrator,
    ).unwrap();

    let mint_address = mint.pubkey();

    // Create sender and recipient token accounts.
    let sender_account = create_token_account(
        &env,
        &mint_address,
        &sender.pubkey(),
    ).unwrap();

    let recipient_account = create_token_account(
        &env,
        &mint_address,
        &recipient.pubkey(),
    ).unwrap();

    // Approve KYC for both accounts.
    for account in [&sender_account, &recipient_account] {
        approve_kyc(
            &env.client,
            &env.payer,
            &administrator,
            &mint_address,
            account,
        ).unwrap();
    }

    println!("PASS: Both accounts passed KYC");

    // Configure confidential transfers.
    for owner in [&sender, &recipient] {
        configure_account_for_confidential_transfers(
            &env.client,
            &env.payer,
            owner,
            &mint_address,
        ).await.unwrap();
    }

    println!("PASS: Confidential accounts configured");

    // Manually approve confidential accounts.
    for account in [&sender_account, &recipient_account] {
        approve_confidential_account(
            &env.client,
            &env.payer,
            &administrator,
            &mint_address,
            account,
        ).unwrap();
    }

    println!("PASS: Confidential accounts approved");

    // Mint one token to the sender.
    mint_tokens(
        &env,
        &mint_address,
        &sender_account,
        &administrator,
        MINT_AMOUNT,
    ).unwrap();

    // Deposit 0.8 tokens into confidential balance.
    deposit_to_confidential(
        &env.client,
        &env.payer,
        &sender,
        &mint_address,
        DEPOSIT_AMOUNT,
        DECIMALS,
    ).await.unwrap();

    apply_pending_balance(
        &env.client,
        &env.payer,
        &sender,
        &mint_address,
    ).await.unwrap();

    println!("PASS: Sender deposited 0.8 tokens");

    // Transfer 0.2 confidential tokens with a 1% fee.
    let signatures = transfer_confidential_with_fee(
        &env.client,
        &env.payer,
        &sender,
        &mint_address,
        &recipient.pubkey(),
        TRANSFER_AMOUNT,
    ).await.unwrap();

    assert!(!signatures.is_empty());

    println!("PASS: Confidential fee transfer executed");
    println!("Transactions: {}", signatures.len());

    // Apply recipient pending balance.
    apply_pending_balance(
        &env.client,
        &env.payer,
        &recipient,
        &mint_address,
    ).await.unwrap();

    println!("PASS: Recipient pending balance applied");

    // Read sender account state.
    let sender_data = env.client
        .get_account(&sender_account)
        .unwrap();

    let sender_state =
        StateWithExtensions::<Account>::unpack(
            &sender_data.data,
        ).unwrap();

    let sender_ext = sender_state
        .get_extension::<ConfidentialTransferAccount>()
        .unwrap();

    let (sender_keys, _) =
        derive_confidential_keys(&sender).unwrap();

    let sender_ciphertext: ElGamalCiphertext =
        PodElGamalCiphertext(
            bytemuck::bytes_of(
                &sender_ext.available_balance,
            ).try_into().unwrap()
        ).try_into().unwrap();

    let sender_confidential = sender_ciphertext
        .decrypt_u32(sender_keys.secret())
        .expect("Unable to decrypt sender balance");

    assert_eq!(
        sender_confidential as u64,
        EXPECTED_SENDER
    );

    assert_eq!(
        sender_state.base.amount,
        MINT_AMOUNT - DEPOSIT_AMOUNT
    );

    println!("PASS: Sender confidential balance = 0.600");
    println!("PASS: Sender public balance = 0.200");

    // Read recipient account state.
    let recipient_data = env.client
        .get_account(&recipient_account)
        .unwrap();

    let recipient_state =
        StateWithExtensions::<Account>::unpack(
            &recipient_data.data,
        ).unwrap();

    let recipient_ext = recipient_state
        .get_extension::<ConfidentialTransferAccount>()
        .unwrap();

    let (recipient_keys, _) =
        derive_confidential_keys(&recipient).unwrap();

    let recipient_ciphertext: ElGamalCiphertext =
        PodElGamalCiphertext(
            bytemuck::bytes_of(
                &recipient_ext.available_balance,
            ).try_into().unwrap()
        ).try_into().unwrap();

    let recipient_confidential = recipient_ciphertext
        .decrypt_u32(recipient_keys.secret())
        .expect("Unable to decrypt recipient balance");

    assert_eq!(
        recipient_confidential as u64,
        EXPECTED_NET
    );

    assert_eq!(recipient_state.base.amount, 0);

    println!("PASS: Recipient confidential balance = 0.198");

    // Verify encrypted withheld transfer fee.
    let fee_ext = recipient_state
        .get_extension::<ConfidentialTransferFeeAmount>()
        .unwrap();

    let (administrator_keys, _) =
        derive_confidential_keys(&administrator).unwrap();

    let fee_ciphertext: ElGamalCiphertext =
        PodElGamalCiphertext(
            bytemuck::bytes_of(
                &fee_ext.withheld_amount,
            ).try_into().unwrap()
        ).try_into().unwrap();

    let withheld_fee = fee_ciphertext
        .decrypt_u32(administrator_keys.secret())
        .expect("Unable to decrypt withheld fee");

    assert_eq!(withheld_fee as u64, EXPECTED_FEE);

    println!("PASS: Encrypted withheld fee = 0.002");

    // Verify conservation of total token supply.
    let total =
        sender_state.base.amount
        + recipient_state.base.amount
        + sender_confidential as u64
        + recipient_confidential as u64
        + withheld_fee as u64;

    assert_eq!(total, MINT_AMOUNT);

    println!("PASS: Total token balance conserved");

    println!("====================================");
    println!("CONFIDENTIAL FEE TRANSFER TEST PASSED");
    println!("====================================");
}
