//! Regression tests ensuring every Error variant is reachable from a test.
//!
//! This is a coverage guard. Each variant of `Error` has exactly one `test_error_<variant>` function
//! (the variant name in lower case) that asserts the call result `Err(Ok(Error::<Variant>))`.
//! `test_every_error_variant_has_exactly_one_matching_test` compares the variants with those
//! functions by name, so a new variant without a test, a duplicated test, a misnamed test, or a
//! test that asserts a different variant all fail `cargo test`. `build.rs` runs a lighter name
//! check so the mistake is also caught when building; it does not run any test.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    token::StellarAssetClient,
    Address, Bytes, BytesN, Env, Symbol, Vec,
};

use crate::{
    Error, ModelSubmission, ScoreAttestation, ScoreGateScoreContract, ScoreGateScoreContractClient,
};

/// Variants that no contract entry point returns yet, so no call can produce them. Their test pins
/// the discriminant (`Error::<Variant> as u32`) instead of a call result. Remove a variant from this
/// list, and give its test a call, as soon as the contract raises it.
const RESERVED_VARIANTS: &[&str] = &["InvalidModelPriorWeight"];

fn setup<'a>() -> (Env, ScoreGateScoreContractClient<'a>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.budget().reset_unlimited();

    let contract_id = env.register_contract(None, ScoreGateScoreContract);
    let client = ScoreGateScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);

    (env, client, admin, service)
}

/// A contract with a real fee token, a funded reserve and a funded challenger, built the way
/// `test_dispute.rs` builds its fixture. Returns the environment, the client and the challenger.
fn dispute_setup<'a>() -> (Env, ScoreGateScoreContractClient<'a>, Address) {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let token = env.register_stellar_asset_contract_v2(Address::generate(&env)).address();
    client.set_fee_token(&Vec::new(&env), &token);

    let minter = StellarAssetClient::new(&env, &token);
    minter.mint(&client.address, &1_000_000);
    let challenger = Address::generate(&env);
    minter.mint(&challenger, &1_000_000);

    (env, client, challenger)
}

/// Commits to `bond` and a 16 byte salt of `salt_byte`, as the sealed-bid dispute flow requires, and
/// returns the salt that opens the commitment.
fn commit_dispute_bond(
    env: &Env,
    client: &ScoreGateScoreContractClient,
    challenger: &Address,
    pair: &Symbol,
    bond: i128,
    salt_byte: u8,
) -> Bytes {
    let mut commitment = Bytes::new(env);
    commitment.extend_from_slice(&bond.to_le_bytes());
    commitment.extend_from_slice(&[salt_byte; 16]);
    let _ = client.try_commit_dispute_bond(challenger, challenger, pair, &commitment);
    Bytes::from_array(env, &[salt_byte; 16])
}

/// A 2-of-2 admin quorum. Returns the two admin signers.
fn admin_quorum(env: &Env, client: &ScoreGateScoreContractClient) -> (Address, Address) {
    let signer_a = Address::generate(env);
    let signer_b = Address::generate(env);
    client.add_admin_signer(&Vec::new(env), &signer_a);
    client.add_admin_signer(&Vec::new(env), &signer_b);
    client.set_admin_threshold(&Vec::new(env), &2);
    (signer_a, signer_b)
}

// ── Error::AlreadyInitialized (1) ──────────────────────────────────────────────

#[test]
fn test_error_alreadyinitialized() {
    let (_env, client, admin, service) = setup();
    client.initialize(&admin, &service);
    let result = client.try_initialize(&admin, &service);
    assert_eq!(result, Err(Ok(Error::AlreadyInitialized)));
}

// ── Error::NotInitialized (2) ──────────────────────────────────────────────────

#[test]
fn test_error_notinitialized() {
    let (_env, client, _admin, _service) = setup();
    let wallet = Address::generate(&_env);
    let pair = symbol_short!("XLM_USDC");
    let result = client.try_get_score(&wallet, &pair);
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}

// ── Error::Unauthorized (3) ────────────────────────────────────────────────────

#[test]
fn test_error_unauthorized() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    // With a 2-of-2 admin quorum, naming the same signer twice is not a valid authorisation.
    let (signer_a, _signer_b) = admin_quorum(&env, &client);
    let mut duplicated = Vec::new(&env);
    duplicated.push_back(signer_a.clone());
    duplicated.push_back(signer_a);

    let result = client.try_set_risk_threshold(&duplicated, &60);
    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

// ── Error::InvalidScore (4) ────────────────────────────────────────────────────

#[test]
fn test_error_invalidscore() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Score > 100 is invalid
    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &101,
        &false,
        &false,
        &1_700_000_000,
        &50,
        &1,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::InvalidScore)));
}

// ── Error::InvalidConfidence (5) ───────────────────────────────────────────────

#[test]
fn test_error_invalidconfidence() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Confidence > 100 is invalid
    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &false,
        &false,
        &1_700_000_000,
        &101,
        &1,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::InvalidConfidence)));
}

// ── Error::ScoreNotFound (6) ───────────────────────────────────────────────────

#[test]
fn test_error_scorenotfound() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let result = client.try_get_score(&wallet, &pair);
    assert_eq!(result, Err(Ok(Error::ScoreNotFound)));
}

// ── Error::ContractPaused (7) ──────────────────────────────────────────────────

#[test]
fn test_error_contractpaused() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    // Pause the contract
    client.pause(&Vec::new(&env));

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Trying to submit while paused should fail
    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &false,
        &false,
        &1_700_000_000,
        &80,
        &1,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::ContractPaused)));
}

// ── Error::NoPendingAdminTransfer (8) ──────────────────────────────────────────

#[test]
fn test_error_nopendingadmintransfer() {
    let (_env, client, _admin, _service) = setup();

    let result = client.try_cancel_admin_transfer(&Vec::new(&_env));
    assert_eq!(result, Err(Ok(Error::NoPendingAdminTransfer)));
}

// ── Error::EmptyBatch (9) ──────────────────────────────────────────────────────

#[test]
fn test_error_emptybatch() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let empty_vec = Vec::new(&env);
    let result = client.try_submit_scores_batch(&Vec::new(&env), &empty_vec);
    assert_eq!(result, Err(Ok(Error::EmptyBatch)));
}

// ── Error::BatchTooLarge (10) ──────────────────────────────────────────────────

#[test]
fn test_error_batchtoolarge() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    // Create a batch larger than MAX_BATCH_SIZE (20)
    let mut batch = Vec::new(&env);
    for i in 0..21 {
        let wallet = Address::generate(&env);
        let pair = symbol_short!("XLM_USDC");
        batch.push_back(crate::ScoreSubmission {
            wallet,
            asset_pair: pair,
            score: 50,
            benford_flag: false,
            ml_flag: false,
            timestamp: 1_700_000_000,
            confidence: 80,
            model_version: 1,
        });
    }

    let result = client.try_submit_scores_batch(&Vec::new(&env), &batch);
    assert_eq!(result, Err(Ok(Error::BatchTooLarge)));
}

// ── Error::ArithmeticOverflow (11) ─────────────────────────────────────────────

#[test]
fn test_error_arithmeticoverflow() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);

    // Twenty pairs with the largest possible weight make the weighted sum overflow.
    let pair_names = [
        "P0", "P1", "P2", "P3", "P4", "P5", "P6", "P7", "P8", "P9", "PA", "PB", "PC", "PD", "PE",
        "PF", "PG", "PH", "PI", "PJ",
    ];
    for (i, name) in pair_names.iter().enumerate() {
        let pair = Symbol::new(&env, name);
        client.set_pair_weight(&Vec::new(&env), &pair, &u32::MAX);
        client.submit_score(
            &Vec::new(&env),
            &wallet,
            &pair,
            &50,
            &false,
            &false,
            &(i as u64 + 1),
            &90,
            &1,
            &None,
        );
    }

    let result = client.try_get_aggregate_score(&wallet);
    assert_eq!(result, Err(Ok(Error::ArithmeticOverflow)));
}

// ── Error::UpgradeAlreadyPending (12) ──────────────────────────────────────────

#[test]
fn test_error_upgradealreadypending() {
    let (env, client, admin, service) = setup();
    env.ledger().with_mut(|l| l.timestamp = 100_000);
    client.initialize(&admin, &service);

    let hash = BytesN::from_array(&env, &[1u8; 32]);
    client.propose_upgrade(&Vec::new(&env), &hash);

    let hash2 = BytesN::from_array(&env, &[2u8; 32]);
    let result = client.try_propose_upgrade(&Vec::new(&env), &hash2);
    assert_eq!(result, Err(Ok(Error::UpgradeAlreadyPending)));
}

// ── Error::NoPendingUpgrade (13) ────────────────────────────────────────────────

#[test]
fn test_error_nopendingupgrade() {
    let (_env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let result = client.try_execute_upgrade(&Vec::new(&_env));
    assert_eq!(result, Err(Ok(Error::NoPendingUpgrade)));
}

// ── Error::InsufficientSigners (14) ────────────────────────────────────────────

#[test]
fn test_error_insufficientsigners() {
    let (env, client, admin, service) = setup();
    env.ledger().with_mut(|l| l.timestamp = 100_000);
    client.initialize(&admin, &service);

    // A 2-of-3 service quorum
    let mut signers = Vec::new(&env);
    for _ in 0..3 {
        let signer = Address::generate(&env);
        client.add_service_signer(&Vec::new(&env), &signer);
        signers.push_back(signer);
    }
    client.set_service_threshold(&Vec::new(&env), &2);

    // Try with only 1 signer when 2 are required
    let mut one = Vec::new(&env);
    one.push_back(signers.get(0).unwrap());

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let result = client.try_submit_score(
        &one,
        &wallet,
        &pair,
        &55,
        &false,
        &false,
        &100_000,
        &80,
        &1,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::InsufficientSigners)));
}

// ── Error::UnauthorizedSigner (15) ─────────────────────────────────────────────

#[test]
fn test_error_unauthorizedsigner() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    // Add a signer to admin multisig
    let signer1 = Address::generate(&env);
    client.add_admin_signer(&Vec::new(&env), &signer1);

    // Try with an unauthorized signer
    let unauthorized = Address::generate(&env);
    let mut signers = Vec::new(&env);
    signers.push_back(unauthorized);

    let result = client.try_set_cooldown(&signers, &3600);
    assert_eq!(result, Err(Ok(Error::UnauthorizedSigner)));
}

// ── Error::InvalidThreshold (16) ───────────────────────────────────────────────

#[test]
fn test_error_invalidthreshold() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    // Try to set service threshold to 0 (must be >= 1)
    let result = client.try_set_service_threshold(&Vec::new(&env), &0);
    assert_eq!(result, Err(Ok(Error::InvalidThreshold)));
}

// ── Error::ServiceSetFull (17) ─────────────────────────────────────────────────

#[test]
fn test_error_servicesetfull() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    // Add MAX_SERVICE_SIGNERS signers
    for _ in 0..crate::constants::MAX_SERVICE_SIGNERS {
        let signer = Address::generate(&env);
        client.add_service_signer(&Vec::new(&env), &signer);
    }

    // Try to add one more
    let extra_signer = Address::generate(&env);
    let result = client.try_add_service_signer(&Vec::new(&env), &extra_signer);
    assert_eq!(result, Err(Ok(Error::ServiceSetFull)));
}

// ── Error::SignerAlreadyInSet (18) ─────────────────────────────────────────────

#[test]
fn test_error_signeralreadyinset() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let signer = Address::generate(&env);
    client.add_service_signer(&Vec::new(&env), &signer);

    // Try to add the same signer again
    let result = client.try_add_service_signer(&Vec::new(&env), &signer);
    assert_eq!(result, Err(Ok(Error::SignerAlreadyInSet)));
}

// ── Error::SignerNotInSet (19) ─────────────────────────────────────────────────

#[test]
fn test_error_signernotinset() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let signer = Address::generate(&env);
    // Try to remove a signer that was never added
    let result = client.try_remove_service_signer(&Vec::new(&env), &signer);
    assert_eq!(result, Err(Ok(Error::SignerNotInSet)));
}

// ── Error::UpgradeNotReady (20) ────────────────────────────────────────────────

#[test]
fn test_error_upgradenotready() {
    let (env, client, admin, service) = setup();
    env.ledger().with_mut(|l| l.timestamp = 100_000);
    client.initialize(&admin, &service);

    let hash = BytesN::from_array(&env, &[1u8; 32]);
    client.propose_upgrade(&Vec::new(&env), &hash);

    // Try to execute before the delay has elapsed
    let result = client.try_execute_upgrade(&Vec::new(&env));
    assert_eq!(result, Err(Ok(Error::UpgradeNotReady)));
}

// ── Error::InvalidUpgradeDelay (21) ────────────────────────────────────────────

#[test]
fn test_error_invalidupgradedelay() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    // Try to set upgrade delay above MAX (1_209_600)
    let result = client.try_set_upgrade_delay(&Vec::new(&env), &(crate::constants::MAX_UPGRADE_DELAY_SECS + 1));
    assert_eq!(result, Err(Ok(Error::InvalidUpgradeDelay)));
}

// ── Error::InvalidStalenessWindow (22) ────────────────────────────────────────

#[test]
fn test_error_invalidstalenesswindow() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    // Staleness window must be reasonable; try with 0
    let result = client.try_set_staleness_window(&Vec::new(&env), &0);
    assert_eq!(result, Err(Ok(Error::InvalidStalenessWindow)));
}

// ── Error::RateLimitExceeded (23) ──────────────────────────────────────────────

#[test]
fn test_error_ratelimitexceeded() {
    let (env, client, admin, service) = setup();
    env.ledger().with_mut(|l| l.timestamp = 100_000);
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Set cooldown to very high value
    client.set_cooldown(&Vec::new(&env), &86_400); // 1 day

    // Submit first score
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &false,
        &false,
        &100_000,
        &80,
        &1,
        &None,
    );

    // Try to submit again immediately
    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &51,
        &false,
        &false,
        &100_001,
        &80,
        &1,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::RateLimitExceeded)));
}

// ── Error::InvalidCooldown (24) ────────────────────────────────────────────────

#[test]
fn test_error_invalidcooldown() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    // Try to set cooldown below MIN (60 seconds)
    let result = client.try_set_cooldown(&Vec::new(&env), &30);
    assert_eq!(result, Err(Ok(Error::InvalidCooldown)));
}

// ── Error::InvalidTimestamp (25) ───────────────────────────────────────────────

#[test]
fn test_error_invalidtimestamp() {
    let (env, client, admin, service) = setup();
    env.ledger().with_mut(|l| l.timestamp = 100_000);
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Submit with a timestamp far in the future (staleness window violation)
    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &false,
        &false,
        &(100_000 + crate::constants::DEFAULT_STALENESS_WINDOW_SECS + 1),
        &80,
        &1,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::InvalidTimestamp)));
}

// ── Error::ServicePubkeyNotSet (26) ────────────────────────────────────────────

#[test]
fn test_error_servicepubkeynotset() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Create an attestation (which requires pubkey to be set for verification)
    let attestation = crate::ScoreAttestation {
        message_hash: BytesN::from_array(&env, &[0u8; 32]),
        signature: Bytes::new(&env),
    };

    // Try to submit with attestation when pubkey is not set
    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &false,
        &false,
        &100_000,
        &80,
        &1,
        &Some(attestation),
    );
    assert_eq!(result, Err(Ok(Error::ServicePubkeyNotSet)));
}

// ── Error::InvalidAttestation (27) ─────────────────────────────────────────────

#[test]
fn test_error_invalidattestation() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);

    // An asset pair longer than the supported length is rejected as an invalid attestation input.
    let oversized_pair = Symbol::new(&env, "PAIR123456");
    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &oversized_pair,
        &42,
        &false,
        &false,
        &1,
        &90,
        &1,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::InvalidAttestation)));
}

// ── Error::InvalidPubkeyLength (28) ────────────────────────────────────────────

#[test]
fn test_error_invalidpubkeylength() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    // A compressed secp256k1 public key is 33 bytes; 32 is the wrong length
    let invalid_pubkey = Bytes::from_array(&env, &[1u8; 32]);
    let result = client.try_set_service_pubkey(&Vec::new(&env), &invalid_pubkey);
    assert_eq!(result, Err(Ok(Error::InvalidPubkeyLength)));
}

// ── Error::InvalidHistoryDepth (29) ────────────────────────────────────────────

#[test]
fn test_error_invalidhistorydepth() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    // Try to set history depth above MAX (50)
    let result = client.try_set_history_max_depth(&Vec::new(&env), &51);
    assert_eq!(result, Err(Ok(Error::InvalidHistoryDepth)));
}

// ── Error::InsufficientConsensus (30) ──────────────────────────────────────────

#[test]
fn test_error_insufficientconsensus() {
    let (env, client, admin, service) = setup();
    env.ledger().with_mut(|l| l.timestamp = 100_000);
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // The only submission has an out-of-range score, so no submission counts towards consensus.
    let mut submissions = Vec::new(&env);
    submissions.push_back(ModelSubmission {
        model_version: 1,
        model: Address::generate(&env),
        score: 101,
        confidence: 80,
        benford_flag: false,
        ml_flag: false,
        attestation: ScoreAttestation {
            commitment: BytesN::from_array(&env, &[0u8; 32]),
            signature: BytesN::from_array(&env, &[0u8; 65]),
            contract_id: BytesN::from_array(&env, &[0u8; 32]),
            contract_version: 0,
            nonce: 0,
        },
    });

    let result =
        client.try_submit_consensus_score(&Vec::new(&env), &wallet, &pair, &submissions, &100_000);
    assert_eq!(result, Err(Ok(Error::InsufficientConsensus)));
}

// ── Error::ConsensusInputEmpty (31) ────────────────────────────────────────────

#[test]
fn test_error_consensusinputempty() {
    let (env, client, admin, service) = setup();
    env.ledger().with_mut(|l| l.timestamp = 100_000);
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let empty = Vec::<ModelSubmission>::new(&env);
    let result =
        client.try_submit_consensus_score(&Vec::new(&env), &wallet, &pair, &empty, &100_000);
    assert_eq!(result, Err(Ok(Error::ConsensusInputEmpty)));
}

// ── Error::InvalidConsensusConfig (32) ─────────────────────────────────────────

#[test]
fn test_error_invalidconsensusconfig() {
    let (_env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    // Try to set invalid consensus config (k=0 is invalid)
    let result = client.try_set_consensus_config(&0, &5);
    assert_eq!(result, Err(Ok(Error::InvalidConsensusConfig)));
}

// ── Error::AdminSetFull (33) ───────────────────────────────────────────────────

#[test]
fn test_error_adminsetfull() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    // Add MAX_ADMIN_SIGNERS signers
    for _ in 0..(crate::constants::MAX_ADMIN_SIGNERS - 1) {
        let signer = Address::generate(&env);
        client.add_admin_signer(&Vec::new(&env), &signer);
    }

    // Try to add one more (should fail as we already have 1 from init + MAX_ADMIN_SIGNERS-1)
    let extra_signer = Address::generate(&env);
    let result = client.try_add_admin_signer(&Vec::new(&env), &extra_signer);
    assert_eq!(result, Err(Ok(Error::AdminSetFull)));
}

// ── Error::AdminSignerNotInSet (34) ────────────────────────────────────────────

#[test]
fn test_error_adminsignernotinset() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let signer = Address::generate(&env);
    // Try to remove an admin signer that was never added
    let result = client.try_remove_admin_signer(&Vec::new(&env), &signer);
    assert_eq!(result, Err(Ok(Error::AdminSignerNotInSet)));
}

// ── Error::InsufficientAdminSigners (35) ───────────────────────────────────────

#[test]
fn test_error_insufficientadminsigners() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    // With a 2-of-2 admin quorum, one signer is not enough.
    let (signer_a, _signer_b) = admin_quorum(&env, &client);
    let mut one = Vec::new(&env);
    one.push_back(signer_a);

    let result = client.try_pause(&one);
    assert_eq!(result, Err(Ok(Error::InsufficientAdminSigners)));
}

// ── Error::CyclicDelegation (36) ───────────────────────────────────────────────

#[test]
fn test_error_cyclicdelegation() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let wallet_a = Address::generate(&env);
    let wallet_b = Address::generate(&env);

    // Delegate A to B
    client.set_score_delegate(&wallet_a, &wallet_b);

    // Try to delegate B to A (creates cycle)
    let result = client.try_set_score_delegate(&wallet_b, &wallet_a);
    assert_eq!(result, Err(Ok(Error::CyclicDelegation)));
}

// ── Error::ScoreEmbargoed (37) ─────────────────────────────────────────────────

#[test]
fn test_error_scoreembargoed() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);

    // Set an embargo on the wallet with no expiry
    client.set_score_embargo(&wallet, &None);

    // Try to get score for embargoed wallet
    let pair = symbol_short!("XLM_USDC");
    let result = client.try_get_score(&wallet, &pair);
    assert_eq!(result, Err(Ok(Error::ScoreEmbargoed)));
}

// ── Error::FeeTokenNotSet (38) ─────────────────────────────────────────────────

#[test]
fn test_error_feetokennotset() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Try to open a dispute when no fee token is configured
    let salt = commit_dispute_bond(&env, &client, &wallet, &pair, 10_000, 1);
    let result = client.try_open_score_dispute(&wallet, &wallet, &pair, &10_000, &salt);
    assert_eq!(result, Err(Ok(Error::FeeTokenNotSet)));
}

// ── Error::QuorumFailureWindowNotElapsed (39) ────────────────────────────────

#[test]
fn test_error_quorumfailurewindownotelapsed() {
    let (env, client, admin, service) = setup();
    env.ledger().with_mut(|l| l.timestamp = 100_000);
    client.initialize(&admin, &service);

    // The variant is returned as `ParameterProposalVetoPeriodEnded`: a parameter change can be
    // vetoed only during the first half of its time lock.
    let value = Bytes::from_array(&env, &crate::constants::MIN_COOLDOWN_SECS.to_be_bytes());
    let proposal_id = client.propose_parameter_change(
        &Vec::from_array(&env, [admin.clone()]),
        &crate::parameter_governance::param_key_cooldown(),
        &value,
    );

    let veto_deadline = 100_000 + crate::constants::DEFAULT_UPGRADE_DELAY_SECS / 2;
    env.ledger().with_mut(|l| l.timestamp = veto_deadline + 1);

    let result =
        client.try_veto_parameter_change(&Vec::from_array(&env, [service.clone()]), &proposal_id);
    assert_eq!(result, Err(Ok(Error::QuorumFailureWindowNotElapsed)));
}

// ── Error::RevealWindowExpired (40) ────────────────────────────────────────────

#[test]
fn test_error_revealwindowexpired() {
    let (env, client, challenger) = dispute_setup();
    let pair = symbol_short!("XLM_USDC");

    // Revealing a bond that was never committed (or whose commitment has lapsed)
    let salt = Bytes::from_array(&env, &[1u8; 16]);
    let result = client.try_open_score_dispute(&challenger, &challenger, &pair, &10_000, &salt);
    assert_eq!(result, Err(Ok(Error::RevealWindowExpired)));
}

// ── Error::CommitmentMismatch (41) ─────────────────────────────────────────────

#[test]
fn test_error_commitmentmismatch() {
    let (env, client, challenger) = dispute_setup();
    let pair = symbol_short!("XLM_USDC");

    // Commit to one salt, then reveal with another
    commit_dispute_bond(&env, &client, &challenger, &pair, 10_000, 1);
    let wrong_salt = Bytes::from_array(&env, &[2u8; 16]);

    let result = client.try_open_score_dispute(&challenger, &challenger, &pair, &10_000, &wrong_salt);
    assert_eq!(result, Err(Ok(Error::CommitmentMismatch)));
}

// ── Error::InvalidFinalityBuffer (42) ──────────────────────────────────────────

#[test]
fn test_error_invalidfinalitybuffer() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    // Try to set finality buffer above MAX (86_400)
    let result = client.try_set_finality_buffer(&Vec::new(&env), &(crate::constants::MAX_FINALITY_BUFFER_SECS + 1));
    assert_eq!(result, Err(Ok(Error::InvalidFinalityBuffer)));
}

// ── Error::NoPendingScore (43) ─────────────────────────────────────────────────

#[test]
fn test_error_nopendingscore() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Try to commit a pending score that doesn't exist
    let result = client.try_commit_pending_score(&wallet, &pair);
    assert_eq!(result, Err(Ok(Error::NoPendingScore)));
}

// ── Error::FinalityWindowNotElapsed (44) ───────────────────────────────────────

#[test]
fn test_error_finalitywindownotelapsed() {
    let (env, client, admin, service) = setup();
    env.ledger().with_mut(|l| l.timestamp = 100_000);
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Set finality buffer
    client.set_finality_buffer(&Vec::new(&env), &300);

    // Submit a score (goes to pending)
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &false,
        &false,
        &100_000,
        &80,
        &1,
        &None,
    );

    // Try to commit immediately (before finality window has passed)
    let result = client.try_commit_pending_score(&wallet, &pair);
    assert_eq!(result, Err(Ok(Error::FinalityWindowNotElapsed)));
}

// ── Error::InvalidDisputeBond (45) ─────────────────────────────────────────────

#[test]
fn test_error_invaliddisputebond() {
    let (env, client, challenger) = dispute_setup();
    let pair = symbol_short!("XLM_USDC");

    // A bond of 0 is invalid
    let salt = commit_dispute_bond(&env, &client, &challenger, &pair, 0, 1);
    let result = client.try_open_score_dispute(&challenger, &challenger, &pair, &0, &salt);
    assert_eq!(result, Err(Ok(Error::InvalidDisputeBond)));
}

// ── Error::DisputeAlreadyOpen (46) ─────────────────────────────────────────────

#[test]
fn test_error_disputealreadyopen() {
    let (env, client, challenger) = dispute_setup();
    let pair = symbol_short!("XLM_USDC");

    // Open a dispute
    let salt = commit_dispute_bond(&env, &client, &challenger, &pair, 10_000, 1);
    client.open_score_dispute(&challenger, &challenger, &pair, &10_000, &salt);

    // Try to open another dispute on the same pair
    let salt = commit_dispute_bond(&env, &client, &challenger, &pair, 10_000, 1);
    let result = client.try_open_score_dispute(&challenger, &challenger, &pair, &10_000, &salt);
    assert_eq!(result, Err(Ok(Error::DisputeAlreadyOpen)));
}

// ── Error::DisputeNotFound (47) ────────────────────────────────────────────────

#[test]
fn test_error_disputenotfound() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Try to resolve a dispute that doesn't exist
    let result = client.try_resolve_dispute_admin(&Vec::new(&env), &wallet, &pair, &60);
    assert_eq!(result, Err(Ok(Error::DisputeNotFound)));
}

// ── Error::DisputeNotYetTimedOut (48) ──────────────────────────────────────────

#[test]
fn test_error_disputenotyettimedout() {
    let (env, client, challenger) = dispute_setup();
    let pair = symbol_short!("XLM_USDC");

    // Open a dispute
    let salt = commit_dispute_bond(&env, &client, &challenger, &pair, 10_000, 1);
    client.open_score_dispute(&challenger, &challenger, &pair, &10_000, &salt);

    // Try to settle by timeout before the deadline
    let result = client.try_resolve_dispute_timeout(&challenger, &pair);
    assert_eq!(result, Err(Ok(Error::DisputeNotYetTimedOut)));
}

// ── Error::InvalidHysteresisMargin (49) ────────────────────────────────────────

#[test]
fn test_error_invalidhysteresismargin() {
    let (_env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    // Try to set hysteresis margin above MAX (50)
    let result = client.try_set_hysteresis_margin(&51);
    assert_eq!(result, Err(Ok(Error::InvalidHysteresisMargin)));
}

// ── Error docs alignment test ──────────────────────────────────────────────────

#[test]
fn test_errors_md_lock_against_discriminants() {
    let errors_rs = include_str!("errors.rs");
    let errors_md = include_str!("../../../docs/errors.md");

    // Parse Error enum variants and their discriminants
    let mut enum_map = std::collections::HashMap::new();
    let mut in_enum = false;

    for line in errors_rs.lines() {
        let line = line.trim();
        if line.starts_with("pub enum Error {") {
            in_enum = true;
            continue;
        }
        if in_enum {
            if line.starts_with('}') {
                in_enum = false;
                continue;
            }
            if line.contains('=') && !line.starts_with("//") {
                let parts: Vec<&str> = line.split('=').collect();
                if parts.len() == 2 {
                    let name = parts[0].trim();
                    let val_str = parts[1].trim().trim_end_matches(',');
                    if let Ok(val) = val_str.parse::<u32>() {
                        enum_map.insert(name.to_string(), val);
                    }
                }
            }
        }
    }

    // Parse impl Error const aliases (e.g. pub const PairPaused: Error = Error::ContractPaused;)
    let mut alias_map = std::collections::HashMap::new();
    for line in errors_rs.lines() {
        let line = line.trim();
        if line.starts_with("pub const ") && line.contains(": Error = Error::") {
            let line_clean = line.trim_start_matches("pub const ");
            let parts: Vec<&str> = line_clean.split(": Error = Error::").collect();
            if parts.len() == 2 {
                let alias_name = parts[0].trim();
                let target_name = parts[1].trim().trim_end_matches(';');
                if let Some(&val) = enum_map.get(target_name) {
                    alias_map.insert(alias_name.to_string(), val);
                }
            }
        }
    }

    // Merge enum variants and aliases into full mapping
    let mut full_map = enum_map;
    for (alias, val) in alias_map {
        full_map.insert(alias, val);
    }

    // Check every table row in docs/errors.md
    for line in errors_md.lines() {
        let line = line.trim();
        if line.starts_with('|') && line.ends_with('|') {
            let cells: Vec<&str> = line.split('|').collect();
            if cells.len() >= 3 {
                let code_cell = cells[1].trim();
                let name_cell = cells[2].trim();
                if let Ok(doc_code) = code_cell.parse::<u32>() {
                    // Extract name inside backticks `Name`
                    if name_cell.starts_with('`') && name_cell.contains('`') {
                        let name = name_cell.trim_matches('`').split_whitespace().next().unwrap_or("");
                        // Skip non-error constants table entry if any
                        if name.starts_with("GATE_CALLER_") {
                            continue;
                        }
                        if let Some(&expected_code) = full_map.get(name) {
                            assert_eq!(
                                doc_code, expected_code,
                                "docs/errors.md code mismatch for {}: doc has {}, errors.rs has {}",
                                name, doc_code, expected_code
                            );
                        }
                    }
                }
            }
        }
    }
}

// ── Error::InvalidModelPriorWeight (50) ────────────────────────────────────────

#[test]
fn test_error_invalidmodelpriorweight() {
    // Reserved: no contract entry point returns this variant yet (see `RESERVED_VARIANTS`), so
    // there is no call to assert. Pin the discriminant so the code is not renumbered silently.
    assert_eq!(Error::InvalidModelPriorWeight as u32, 50);
}

// ── Coverage guard ─────────────────────────────────────────────────────────────

/// Every variant of `Error` must have exactly one `test_error_<variant>` function, and that function
/// must assert the variant itself. Counting tests is not enough: two tests for one variant keep the
/// totals equal while leaving another variant unchecked.
#[test]
fn test_every_error_variant_has_exactly_one_matching_test() {
    let errors_rs = include_str!("errors.rs");
    let this_file = include_str!("error_coverage.rs");

    // Every variant of `Error`, in declaration order.
    let mut variants: std::vec::Vec<&str> = std::vec::Vec::new();
    let mut in_enum = false;
    for line in errors_rs.lines() {
        let line = line.trim();
        if line.starts_with("pub enum Error {") {
            in_enum = true;
            continue;
        }
        if !in_enum {
            continue;
        }
        if line.starts_with('}') {
            break;
        }
        if let Some((name, discriminant)) = line.split_once('=') {
            if discriminant.trim().trim_end_matches(',').parse::<u32>().is_ok() {
                variants.push(name.trim());
            }
        }
    }
    assert!(!variants.is_empty(), "no Error variants found in errors.rs");

    // Every `test_error_*` function in this file with its body. Test functions start in column 0
    // and end at the next line that is a lone closing brace.
    let mut tests: std::vec::Vec<(std::string::String, std::string::String)> = std::vec::Vec::new();
    let mut current: Option<usize> = None;
    for line in this_file.lines() {
        if let Some(rest) = line.strip_prefix("fn test_error_") {
            let name = rest.split('(').next().unwrap_or("");
            tests.push((std::format!("test_error_{}", name), std::string::String::new()));
            current = Some(tests.len() - 1);
            continue;
        }
        if let Some(index) = current {
            if line == "}" {
                current = None;
            } else {
                tests[index].1.push_str(line);
                tests[index].1.push('\n');
            }
        }
    }

    let mut problems: std::vec::Vec<std::string::String> = std::vec::Vec::new();

    for variant in &variants {
        let expected_name = std::format!("test_error_{}", variant.to_lowercase());
        let matching: std::vec::Vec<&(std::string::String, std::string::String)> =
            tests.iter().filter(|(name, _)| *name == expected_name).collect();
        if matching.len() != 1 {
            problems.push(std::format!(
                "Error::{}: expected one test named {}, found {}",
                variant,
                expected_name,
                matching.len()
            ));
            continue;
        }

        let needle = if RESERVED_VARIANTS.contains(variant) {
            std::format!("Error::{} as u32", variant)
        } else {
            std::format!("Err(Ok(Error::{}))", variant)
        };
        if !matching[0].1.contains(&needle) {
            problems.push(std::format!("{} does not assert `{}`", expected_name, needle));
        }
    }

    // A test that matches no variant is a typo or a left-over from a removed variant.
    for (name, _) in &tests {
        if !variants.iter().any(|variant| std::format!("test_error_{}", variant.to_lowercase()) == *name) {
            problems.push(std::format!("{} does not match any Error variant", name));
        }
    }

    for reserved in RESERVED_VARIANTS {
        if !variants.contains(reserved) {
            problems.push(std::format!("RESERVED_VARIANTS lists {}, which is not an Error variant", reserved));
        }
    }

    assert!(problems.is_empty(), "error coverage problems:\n{}", problems.join("\n"));
}
