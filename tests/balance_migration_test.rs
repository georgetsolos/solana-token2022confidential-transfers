mod common;

use common::{
    TestEnv,
    create_token_account,
    mint_tokens,
};

use conf_balances_examples::{
    extensions::create_regulated_mint,
    migration::{
        create_regulated_confidential_mint,
        migrate_v1_to_v2,
    },
    kyc::{approve_kyc, revoke_kyc},
};

use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};

use spl_token_2022::{
    extension::StateWithExtensions,
    state::{Account, Mint},
};

fn token_balance(
    env: &TestEnv,
    address: &Pubkey,
) -> u64 {
    let data = env.client
        .get_account(address)
        .unwrap();

    StateWithExtensions::<Account>::unpack(
        &data.data,
    ).unwrap().base.amount
}

fn mint_supply(
    env: &TestEnv,
    address: &Pubkey,
) -> u64 {
    let data = env.client
        .get_account(address)
        .unwrap();

    StateWithExtensions::<Mint>::unpack(
        &data.data,
    ).unwrap().base.supply
}

#[test]
fn test_atomic_v1_to_v2_migration() {
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
    let unauthorized = Keypair::new();

    const INITIAL_AMOUNT: u64 = 10_000_000_000;
    const MIGRATION_AMOUNT: u64 = 6_000_000_000;

    println!("====================================");
    println!("ATOMIC V1 TO V2 MIGRATION TEST");
    println!("====================================");

    // Create original V1 mint.
    let v1 = create_regulated_mint(
        &env.client,
        &env.payer,
        &administrator.pubkey(),
    ).unwrap();

    // Create replacement V2 mint.
    let v2 = create_regulated_confidential_mint(
        &env.client,
        &env.payer,
        &administrator,
    ).unwrap();

    let v1_address = v1.pubkey();
    let v2_address = v2.pubkey();

    assert_ne!(v1_address, v2_address);

    println!("PASS: Both mint versions created");

    // Create token accounts for the same owner.
    let source = create_token_account(
        &env,
        &v1_address,
        &owner.pubkey(),
    ).unwrap();

    let destination = create_token_account(
        &env,
        &v2_address,
        &owner.pubkey(),
    ).unwrap();

    // Approve both accounts.
    approve_kyc(
        &env.client,
        &env.payer,
        &administrator,
        &v1_address,
        &source,
    ).unwrap();

    approve_kyc(
        &env.client,
        &env.payer,
        &administrator,
        &v2_address,
        &destination,
    ).unwrap();

    println!("PASS: Both accounts passed KYC");

    // Mint 10 original tokens.
    mint_tokens(
        &env,
        &v1_address,
        &source,
        &administrator,
        INITIAL_AMOUNT,
    ).unwrap();

    // Capture balances and mint supplies.
    let snapshot = || {
        (
            token_balance(&env, &source),
            token_balance(&env, &destination),
            mint_supply(&env, &v1_address),
            mint_supply(&env, &v2_address),
        )
    };

    let before = snapshot();

    assert_eq!(
        before,
        (INITIAL_AMOUNT, 0, INITIAL_AMOUNT, 0)
    );

    println!("PASS: Initial V1 balance = 10");
    println!("PASS: Initial V2 balance = 0");

    // Reject unauthorized administrator.
    let result = migrate_v1_to_v2(
        &env.client,
        &env.payer,
        &owner,
        &unauthorized,
        &v1_address,
        &v2_address,
        MIGRATION_AMOUNT,
    );

    assert!(
        result.expect_err("Must reject unauthorized admin")
            .to_string()
            .contains("Unauthorized migration administrator")
    );

    assert_eq!(snapshot(), before);

    println!("PASS: Unauthorized administrator rejected");

    // Reject excessive migration.
    let result = migrate_v1_to_v2(
        &env.client,
        &env.payer,
        &owner,
        &administrator,
        &v1_address,
        &v2_address,
        11_000_000_000,
    );

    assert!(
        result.expect_err("Must reject excessive migration")
            .to_string()
            .contains("Insufficient V1 token balance")
    );

    assert_eq!(snapshot(), before);

    println!("PASS: Insufficient balance rejected");

    // Freeze the destination account.
    revoke_kyc(
        &env.client,
        &env.payer,
        &administrator,
        &v2_address,
        &destination,
    ).unwrap();

    let result = migrate_v1_to_v2(
        &env.client,
        &env.payer,
        &owner,
        &administrator,
        &v1_address,
        &v2_address,
        MIGRATION_AMOUNT,
    );

    assert!(
        result.expect_err("Must reject frozen destination")
            .to_string()
            .contains("Both accounts require KYC approval")
    );

    assert_eq!(snapshot(), before);

    println!("PASS: Frozen destination rejected");
    println!("PASS: Balances unchanged after rejected requests");

    // Restore destination KYC approval.
    approve_kyc(
        &env.client,
        &env.payer,
        &administrator,
        &v2_address,
        &destination,
    ).unwrap();

    // Perform valid migration.
    let signature = migrate_v1_to_v2(
        &env.client,
        &env.payer,
        &owner,
        &administrator,
        &v1_address,
        &v2_address,
        MIGRATION_AMOUNT,
    ).unwrap();

    println!("Migration transaction: {}", signature);

    let after = snapshot();

    assert_eq!(
        after,
        (
            4_000_000_000,
            6_000_000_000,
            4_000_000_000,
            6_000_000_000,
        )
    );

    println!("PASS: Remaining V1 balance = 4");
    println!("PASS: New V2 balance = 6");
    println!("PASS: V1 mint supply = 4");
    println!("PASS: V2 mint supply = 6");

    assert_eq!(after.0 + after.1, INITIAL_AMOUNT);
    assert_eq!(after.2 + after.3, INITIAL_AMOUNT);

    println!("PASS: Total token supply conserved");

    println!("====================================");
    println!("ATOMIC BALANCE MIGRATION PASSED");
    println!("====================================");
}
