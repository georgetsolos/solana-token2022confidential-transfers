//! Integration tests for regulated Token-2022 extensions.

mod common;

use common::{
    TestEnv,
    create_token_account,
    mint_tokens,
};

use conf_balances_examples::{
    extensions::{
        create_regulated_mint,
        regulated_mint_size,
        DECIMALS,
    },
    kyc::{
        approve_kyc,
        revoke_kyc,
        is_account_frozen,
    },
    fee_transfer::{
        calculate_current_fee,
        transfer_with_fee,
    },
};

use solana_sdk::signature::{Keypair, Signer};

use spl_token_2022::{
    extension::{
        BaseStateWithExtensions,
        StateWithExtensions,
        ExtensionType,
        transfer_fee::TransferFeeAmount,
    },
    state::{Mint, Account},
};

#[test]
fn test_regulated_mint_kyc_and_fees() {
    let env = TestEnv::new();

    env.airdrop_if_needed(
        &env.payer_pubkey(),
        10_000_000_000,
    ).expect("Failed to fund payer");

    let administrator = Keypair::new();
    let sender = Keypair::new();
    let recipient = Keypair::new();

    // 1. Create regulated mint.
    let mint = create_regulated_mint(
        &env.client,
        &env.payer,
        &administrator.pubkey(),
    ).expect("Failed to create regulated mint");

    // 2. Inspect mint extensions.
    let mint_account = env.client
        .get_account(&mint.pubkey())
        .expect("Failed to read mint");

    assert_eq!(
        mint_account.owner,
        spl_token_2022::id(),
    );

    assert_eq!(
        mint_account.data.len(),
        regulated_mint_size().unwrap(),
    );

    let mint_state =
        StateWithExtensions::<Mint>::unpack(
            &mint_account.data,
        ).unwrap();

    let extensions =
        mint_state.get_extension_types().unwrap();

    for extension in [
        ExtensionType::TransferFeeConfig,
        ExtensionType::MetadataPointer,
        ExtensionType::DefaultAccountState,
        ExtensionType::MintCloseAuthority,
    ] {
        assert!(extensions.contains(&extension));
        println!("Verified extension: {:?}", extension);
    }

    assert_eq!(mint_state.base.decimals, DECIMALS);

    // 3. Create sender and recipient token accounts.
    let source = create_token_account(
        &env,
        &mint.pubkey(),
        &sender.pubkey(),
    ).unwrap();

    let destination = create_token_account(
        &env,
        &mint.pubkey(),
        &recipient.pubkey(),
    ).unwrap();

    // 4. Verify default frozen state.
    assert!(
        is_account_frozen(&env.client, &source).unwrap()
    );

    assert!(
        is_account_frozen(&env.client, &destination).unwrap()
    );

    println!("PASS: New accounts are frozen");

    // 5. Approve sender.
    approve_kyc(
        &env.client,
        &env.payer,
        &administrator,
        &mint.pubkey(),
        &source,
    ).unwrap();

    assert!(
        !is_account_frozen(&env.client, &source).unwrap()
    );

    // 6. Mint 10 tokens to sender.
    mint_tokens(
        &env,
        &mint.pubkey(),
        &source,
        &administrator,
        10_000_000_000,
    ).unwrap();

    // 7. Verify current epoch fee.
    let amount = 2_000_000_000;

    let fee = calculate_current_fee(
        &env.client,
        &mint.pubkey(),
        amount,
    ).unwrap();

    assert_eq!(fee, 20_000_000);

    println!("PASS: Current epoch fee is correct");

    // 8. Transfer to frozen recipient must fail.
    let blocked = transfer_with_fee(
        &env.client,
        &env.payer,
        &sender,
        &source,
        &mint.pubkey(),
        &destination,
        amount,
    );

    assert!(
        blocked.is_err(),
        "Transfer to frozen account should fail"
    );

    println!("PASS: Frozen recipient cannot receive transfer");

    // 9. Approve recipient.
    approve_kyc(
        &env.client,
        &env.payer,
        &administrator,
        &mint.pubkey(),
        &destination,
    ).unwrap();

    // 10. Execute fee-aware transfer.
    transfer_with_fee(
        &env.client,
        &env.payer,
        &sender,
        &source,
        &mint.pubkey(),
        &destination,
        amount,
    ).unwrap();

    // 11. Verify actual on-chain balances.
    let source_data =
        env.client.get_account(&source).unwrap();

    let destination_data =
        env.client.get_account(&destination).unwrap();

    let source_state =
        StateWithExtensions::<Account>::unpack(
            &source_data.data,
        ).unwrap();

    let destination_state =
        StateWithExtensions::<Account>::unpack(
            &destination_data.data,
        ).unwrap();

    assert_eq!(
        source_state.base.amount,
        8_000_000_000,
    );

    assert_eq!(
        destination_state.base.amount,
        1_980_000_000,
    );

    let withheld = destination_state
        .get_extension::<TransferFeeAmount>()
        .unwrap();

    assert_eq!(
        u64::from(withheld.withheld_amount),
        20_000_000,
    );

    println!("PASS: Sender balance is 8 tokens");
    println!("PASS: Recipient balance is 1.98 tokens");
    println!("PASS: Withheld fee is 0.02 tokens");

    // 12. Revoke recipient approval.
    revoke_kyc(
        &env.client,
        &env.payer,
        &administrator,
        &mint.pubkey(),
        &destination,
    ).unwrap();

    assert!(
        is_account_frozen(
            &env.client,
            &destination,
        ).unwrap()
    );

    println!("PASS: KYC approval revoked");
    println!("test_regulated_mint_kyc_and_fees PASSED");
}
