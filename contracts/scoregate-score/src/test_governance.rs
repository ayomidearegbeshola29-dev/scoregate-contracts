use crate::{Error, ParamValue, ScoreGateScoreContract, ScoreGateScoreContractClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger as _},
    Address, Bytes, BytesN, Env, Symbol, TryFromVal, Vec,
};

fn setup() -> (Env, ScoreGateScoreContractClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, ScoreGateScoreContract);
    let client = ScoreGateScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    (env, client, admin, service)
}

// ── Issue #220: apply_param_change cooldown handling & param_change_applied event ──

#[test]
fn test_apply_param_change_cooldown_and_all_keys_emit_audit_event() {
    let (env, client, _admin, _service) = setup();
    let signers = Vec::new(&env);

    // 1. risk_thr
    client.set_risk_threshold(&signers, &82);
    env.ledger().with_mut(|l| l.timestamp += 86_401);
    let key_risk = symbol_short!("risk_thr");
    client.apply_param_change(&key_risk);
    assert_eq!(client.get_risk_threshold(), 82);

    // 2. hist_dep
    client.set_history_max_depth(&signers, &16);
    env.ledger().with_mut(|l| l.timestamp += 86_401);
    let key_hist = symbol_short!("hist_dep");
    client.apply_param_change(&key_hist);
    assert_eq!(client.get_history_max_depth(), 16);

    // 3. upg_dly
    client.set_upgrade_delay(&signers, &259_200);
    env.ledger().with_mut(|l| l.timestamp += 86_401);
    let key_upg = symbol_short!("upg_dly");
    client.apply_param_change(&key_upg);
    assert_eq!(client.get_upgrade_delay(), 259_200);

    // 4. cooldown
    client.propose_cooldown_change(&signers, &1_800);
    env.ledger().with_mut(|l| l.timestamp += 86_401);
    let key_cooldown = symbol_short!("cooldown");
    client.apply_param_change(&key_cooldown);
    assert_eq!(client.get_cooldown(), 1_800);

    // 5. stale_w
    client.set_staleness_window(&signers, &7_200);
    env.ledger().with_mut(|l| l.timestamp += 86_401);
    let key_stale = symbol_short!("stale_w");
    client.apply_param_change(&key_stale);
    assert_eq!(client.get_staleness_window(), 7_200);

    // Verify all 5 `pc_appl` (param_change_applied) events were emitted with (key, ParamValue).
    let applied_events: std::vec::Vec<_> = env
        .events()
        .all()
        .iter()
        .filter(|e| {
            e.1.get(0).and_then(|v| Symbol::try_from_val(&env, &v).ok())
                == Some(symbol_short!("pc_appl"))
        })
        .collect();
    assert_eq!(applied_events.len(), 5);

    let ev0: (Symbol, ParamValue) = TryFromVal::try_from_val(&env, &applied_events[0].2).unwrap();
    let ev1: (Symbol, ParamValue) = TryFromVal::try_from_val(&env, &applied_events[1].2).unwrap();
    let ev2: (Symbol, ParamValue) = TryFromVal::try_from_val(&env, &applied_events[2].2).unwrap();
    let ev3: (Symbol, ParamValue) = TryFromVal::try_from_val(&env, &applied_events[3].2).unwrap();
    let ev4: (Symbol, ParamValue) = TryFromVal::try_from_val(&env, &applied_events[4].2).unwrap();

    assert_eq!(ev0, (key_risk, ParamValue::U32(82)));
    assert_eq!(ev1, (key_hist, ParamValue::U32(16)));
    assert_eq!(ev2, (key_upg, ParamValue::U64(259_200)));
    assert_eq!(ev3, (key_cooldown, ParamValue::U64(1_800)));
    assert_eq!(ev4, (key_stale, ParamValue::U64(7_200)));
}

#[test]
fn test_apply_param_change_seeded_cooldown_proposal() {
    let (env, client, _admin, _service) = setup();
    let key = symbol_short!("cooldown");

    env.as_contract(&client.address, || {
        crate::storage::set_pending_param_change(
            &env,
            &key,
            &crate::types::ParamChangeProposal {
                new_value: ParamValue::U64(900),
                proposed_at: 0,
                apply_after: 100,
            },
        );
    });

    env.ledger().with_mut(|l| l.timestamp = 101);
    client.apply_param_change(&key);
    assert_eq!(client.get_cooldown(), 900);
}

// ── Issue #219: veto_policy_bundle multisig auth & cancellation ──────────────

#[test]
fn test_veto_policy_bundle_enforces_multisig_admin_auth() {
    let (env, client, _admin, _service) = setup();
    let admin1 = Address::generate(&env);
    let admin2 = Address::generate(&env);
    let outsider = Address::generate(&env);

    client.add_admin_signer(&Vec::new(&env), &admin1);
    client.add_admin_signer(&Vec::new(&env), &admin2);
    client.set_admin_threshold(&Vec::new(&env), &2);

    let mut two_signers = Vec::new(&env);
    two_signers.push_back(admin1.clone());
    two_signers.push_back(admin2.clone());

    client.propose_policy_bundle(&two_signers, &85, &1_800);

    let bundle_id = symbol_short!("pol_bdl");

    // Insufficient signers fails.
    let mut one_signer = Vec::new(&env);
    one_signer.push_back(admin1.clone());
    assert_eq!(
        client.try_veto_policy_bundle(&one_signer, &bundle_id),
        Err(Ok(Error::InsufficientAdminSigners))
    );

    // Non-member signer fails.
    let mut bad_signers = Vec::new(&env);
    bad_signers.push_back(admin1.clone());
    bad_signers.push_back(outsider);
    assert_eq!(
        client.try_veto_policy_bundle(&bad_signers, &bundle_id),
        Err(Ok(Error::AdminSignerNotInSet))
    );

    // Valid quorum succeeds and revokes the pending policy bundle.
    client.veto_policy_bundle(&two_signers, &bundle_id);
    env.ledger().with_mut(|l| l.timestamp += 86_401);
    assert_eq!(client.try_apply_policy_bundle(), Err(Ok(Error::NoPendingUpgrade)));
}

// ── Issue #218: Mandatory reviewers enforcement across governance pipelines ──

#[test]
fn test_mandatory_reviewers_enforced_on_apply_param_change() {
    let (env, client, _admin, _service) = setup();
    let rev1 = Address::generate(&env);
    let rev2 = Address::generate(&env);
    let non_rev = Address::generate(&env);

    let mut reviewers = Vec::new(&env);
    reviewers.push_back(rev1.clone());
    reviewers.push_back(rev2.clone());
    client.set_mandatory_reviewers(&reviewers);

    let signers = Vec::new(&env);
    let key = symbol_short!("risk_thr");
    client.set_risk_threshold(&signers, &88);
    env.ledger().with_mut(|l| l.timestamp += 86_401);

    // Fails when no mandatory reviewers have approved.
    assert_eq!(
        client.try_apply_param_change(&key),
        Err(Ok(Error::MissingMandatoryReviewerApprovals))
    );

    // Non-reviewer cannot approve.
    assert_eq!(
        client.try_approve_proposal_review(&non_rev, &key),
        Err(Ok(Error::ReviewerNotFound))
    );

    // Partial approval (1 of 2) still blocks execution.
    client.approve_proposal_review(&rev1, &key);
    assert_eq!(
        client.try_apply_param_change(&key),
        Err(Ok(Error::MissingMandatoryReviewerApprovals))
    );

    // Second mandatory reviewer approves -> apply_param_change succeeds.
    client.approve_proposal_review(&rev2, &key);
    assert_eq!(client.get_proposal_review_approvals(&key).len(), 2);

    client.apply_param_change(&key);
    assert_eq!(client.get_risk_threshold(), 88);

    // Approvals are cleared after application.
    assert!(client.get_proposal_review_approvals(&key).is_empty());
}

#[test]
fn test_mandatory_reviewers_enforced_on_policy_bundle() {
    let (env, client, _admin, _service) = setup();
    let rev1 = Address::generate(&env);
    let rev2 = Address::generate(&env);

    let mut reviewers = Vec::new(&env);
    reviewers.push_back(rev1.clone());
    reviewers.push_back(rev2.clone());
    client.set_mandatory_reviewers(&reviewers);

    let signers = Vec::new(&env);
    let target = symbol_short!("pol_bdl");
    client.propose_policy_bundle(&signers, &90, &1_200);
    env.ledger().with_mut(|l| l.timestamp += 86_401);

    // Unapproved bundle cannot be applied.
    assert_eq!(client.try_apply_policy_bundle(), Err(Ok(Error::MissingMandatoryReviewerApprovals)));

    client.approve_proposal_review(&rev1, &target);
    assert_eq!(client.try_apply_policy_bundle(), Err(Ok(Error::MissingMandatoryReviewerApprovals)));

    client.approve_proposal_review(&rev2, &target);
    client.apply_policy_bundle();

    assert_eq!(client.get_risk_threshold(), 90);
    assert_eq!(client.get_cooldown(), 1_200);
    assert!(client.get_proposal_review_approvals(&target).is_empty());
}

#[test]
fn test_mandatory_reviewers_enforced_on_parameter_governance_and_upgrade() {
    let (env, client, _admin, _service) = setup();
    let rev1 = Address::generate(&env);

    let mut reviewers = Vec::new(&env);
    reviewers.push_back(rev1.clone());
    client.set_mandatory_reviewers(&reviewers);

    let signers = Vec::new(&env);
    let param_key = symbol_short!("cooldown");
    let new_val = Bytes::from_slice(&env, &1_800u64.to_be_bytes());

    let prop_id = client.propose_parameter_change(&signers, &param_key, &new_val);
    let delay = client.get_upgrade_delay();
    env.ledger().with_mut(|l| l.timestamp += delay + 1);

    // Blocked without mandatory reviewer approval.
    assert_eq!(
        client.try_execute_parameter_change(&signers, &prop_id),
        Err(Ok(Error::MissingMandatoryReviewerApprovals))
    );

    // Reviewer approves -> execution succeeds.
    client.approve_proposal_review(&rev1, &param_key);
    client.execute_parameter_change(&signers, &prop_id);
    assert_eq!(client.get_cooldown(), 1_800);
    assert!(client.get_proposal_review_approvals(&param_key).is_empty());

    // Upgrade proposal also checks mandatory reviewer approval before WASM swap.
    let wasm_hash = BytesN::from_array(&env, &[7u8; 32]);
    client.propose_upgrade(&signers, &wasm_hash);
    env.ledger().with_mut(|l| l.timestamp += delay + 1);

    assert_eq!(
        client.try_execute_upgrade(&signers),
        Err(Ok(Error::MissingMandatoryReviewerApprovals))
    );
}
