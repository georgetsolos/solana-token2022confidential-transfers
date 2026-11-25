//! Integration tests for the regulated confidential mint.

mod common;

use common::{
    TestEnv,
    create_token_account,
    mint_tokens,
};

use conf_balances_examples::{
    migration::{
        create_regulated_confidential_mint,
        regulated_confidential_mint_size,
        V2_EXTENSIONS,
    },
    kyc::{
        approve_kyc,
        is_account_frozen,
    },
    fee_transfer::transfer_with_fee,
    extensions::DECIMALS,
};

use solana_sdk::signature::{Keypair, Signer};

use spl_token_2022::{
    extension::{
        confidential_transfer::ConfidentialTransferMint,
        permanent_delegate::PermanentDelegate,
        BaseStateWithExtensions,
        StateWithExtensions,
    },
    state::{Account, Mint},
};

#[test]
fn test_regulated_confidential_mint_v2() {
    let env = TestEnv::new();

    assert!(
        env.is_local,
        "This test must run on Localnet"
    );

    env.airdrop_if_needed(
        &env.payer_pubkey(),
        10_000_000_000,
    ).expect("Failed to fund payer");

    let administrator = Keypair::new();
    let owner = Keypair::new();
    let recipient = Keypair::new();

    // Create the V2 mint.
    let mint = create_regulated_confidential_mint(
        &env.client,
        &env.payer,
        &administrator,
    ).expect("Failed to create V2 mint");

    let mint_address = mint.pubkey();

    let mint_account = env.client
        .get_account(&mint_address)
        .unwrap();

    assert_eq!(
        mint_account.owner,
        spl_token_2022::id()
    );

    assert_eq!(
        mint_account.data.len(),
        regulated_confidential_mint_size().unwrap()
    );

    let mint_state =
        StateWithExtensions::<Mint>::unpack(
            &mint_account.data,
        ).unwrap();

    assert_eq!(
        mint_state.base.decimals,
        DECIMALS
    );

    let extensions =
        mint_state.get_extension_types().unwrap();

    assert_eq!(extensions.len(), 7);

    for extension in V2_EXTENSIONS {
        assert!(
            extensions.contains(&extension),
            "Missing extension: {:?}",
            extension
        );

        println!(
            "PASS: Extension {:?}",
            extension
        );
    }

    // Verify manual confidential approval.
    let confidential = mint_state
        .get_extension::<ConfidentialTransferMint>()
        .unwrap();

    assert!(
        !bool::from(
            confidential.auto_approve_new_accounts
        )
    );

    println!("PASS: Manual confidential approval enabled");

    // Verify Permanent Delegate.
    let delegate = mint_state
        .get_extension::<PermanentDelegate>()
        .unwrap();

    assert_eq!(
        Option::<solana_sdk::pubkey::Pubkey>::from(
            delegate.delegate
        ),
        Some(administrator.pubkey())
    );

    println!("PASS: Permanent Delegate authority verified");

    // Create token accounts.
    let source = create_token_account(
        &env,
        &mint_address,
        &owner.pubkey(),
    ).unwrap();

    let destination = create_token_account(
        &env,
        &mint_address,
        &recipient.pubkey(),
    ).unwrap();

    assert!(
        is_account_frozen(
            &env.client,
            &source,
        ).unwrap()
    );

    assert!(
        is_account_frozen(
            &env.client,
            &destination,
        ).unwrap()
    );

    println!("PASS: V2 accounts start frozen");

    // Approve KYC for both accounts.
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
        &destination,
    ).unwrap();

    // Mint 10 tokens to the owner.
    mint_tokens(
        &env,
        &mint_address,
        &source,
        &administrator,
        10_000_000_000,
    ).unwrap();

    // An unrelated signer must not transfer the tokens.
    let unauthorized = transfer_with_fee(
        &env.client,
        &env.payer,
        &recipient,
        &source,
        &mint_address,
        &destination,
        2_000_000_000,
    );

    assert!(
        unauthorized.is_err(),
        "Unauthorized transfer should fail"
    );

    println!("PASS: Unauthorized transfer rejected");

    // Permanent Delegate transfers 2 public tokens.
    // The source token owner does not sign.
    transfer_with_fee(
        &env.client,
        &env.payer,
        &administrator,
        &source,
        &mint_address,
        &destination,
        2_000_000_000,
    ).expect("Permanent Delegate transfer failed");

    println!("PASS: Permanent Delegate transfer succeeded");

    // Verify resulting balances.
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
        8_000_000_000
    );

    assert_eq!(
        destination_state.base.amount,
        1_980_000_000
    );

    println!("PASS: Source balance is 8 tokens");
    println!("PASS: Destination balance is 1.98 tokens");

    println!("====================================");
    println!("V2 REGULATED MINT TEST PASSED");
    println!("====================================");
}
