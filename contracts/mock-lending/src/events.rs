#![cfg_attr(target_family = "wasm", allow(dead_code))]

//! Audit events for the mock lending fixture (Issue #119).
//!
//! Mirrors the topic/version convention used by `scoregate-score`'s own
//! `events.rs` (and `mock-amm`'s equivalent module): a short
//! `symbol_short!` topic, an explicit `EVENT_VERSION` in the topic array,
//! and a small named helper per event so call sites stay readable.

use soroban_sdk::{symbol_short, Address, Env, Symbol};

/// All events emitted by this contract carry an explicit schema version in
/// their topic array. Bump this if an event's data shape changes in a way
/// that isn't purely additive.
pub const EVENT_VERSION: u32 = 1;

/// Emitted when `borrow` clears the risk gate and completes.
pub fn borrow_executed(env: &Env, user: &Address, asset_pair: &Symbol, amount: i128) {
    env.events().publish(
        (symbol_short!("brw_exc"), EVENT_VERSION, asset_pair.clone()),
        (user.clone(), amount),
    );
}

/// Emitted when the admin updates the borrow gate config via
/// `set_borrow_gate_config`.
pub fn gate_config_updated(env: &Env, gate_threshold: u32, min_confidence: u32) {
    env.events()
        .publish((symbol_short!("gate_cfg"), EVENT_VERSION), (gate_threshold, min_confidence));
}

/// Emitted when the admin rotates the configured ScoreGate oracle via
/// `set_risk_oracle`.
pub fn oracle_updated(env: &Env, new_oracle: &Address) {
    env.events().publish((symbol_short!("orcl_upd"), EVENT_VERSION), new_oracle.clone());
}

/// Emitted when `refresh_oracle_capabilities` re-probes the currently
/// configured oracle (e.g. after it was upgraded in place at the same
/// address) and updates the cached expanded-score flag.
pub fn capabilities_refreshed(env: &Env, expanded_score: bool) {
    env.events().publish((symbol_short!("cap_rfsh"), EVENT_VERSION), expanded_score);
}
