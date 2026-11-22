#![cfg_attr(target_family = "wasm", allow(dead_code))]

//! Audit events for the mock AMM fixture (Issue #119).
//!
//! Mirrors the topic/version convention used by `scoregate-score`'s own
//! `events.rs`: a short `symbol_short!` topic, an explicit `EVENT_VERSION`
//! in the topic array, and a small named helper per event so call sites
//! stay readable.

use soroban_sdk::{symbol_short, Address, Env, Symbol};

/// All events emitted by this contract carry an explicit schema version in
/// their topic array. Bump this if an event's data shape changes in a way
/// that isn't purely additive.
pub const EVENT_VERSION: u32 = 1;

/// Topic symbol used by `swap_executed`.
pub const SWAP_EXECUTED_TOPIC: Symbol = symbol_short!("swap_exc");

/// Topic symbol used by `liquidity_provided`.
pub const LIQUIDITY_PROVIDED_TOPIC: Symbol = symbol_short!("liq_prov");

/// Emitted when `swap` clears the risk gate and completes.
pub fn swap_executed(env: &Env, user: &Address, asset_pair: &Symbol, amount: i128) {
    env.events().publish(
        (SWAP_EXECUTED_TOPIC, EVENT_VERSION, asset_pair.clone()),
        (user.clone(), amount),
    );
}

/// Emitted when `provide_liquidity_gated` clears the risk gate and completes.
pub fn liquidity_provided(env: &Env, provider: &Address, asset_pair: &Symbol, amount: i128) {
    env.events().publish(
        (LIQUIDITY_PROVIDED_TOPIC, EVENT_VERSION, asset_pair.clone()),
        (provider.clone(), amount),
    );
}

/// Emitted when the fixture administrator is rotated via `set_admin`.
pub fn admin_updated(env: &Env, new_admin: &Address) {
    env.events().publish((symbol_short!("adm_upd"), EVENT_VERSION), new_admin.clone());
}

/// Emitted when the protocol pause state is updated via `set_paused`.
pub fn paused_updated(env: &Env, paused: bool) {
    env.events().publish((symbol_short!("pause_upd"), EVENT_VERSION), paused);
}

/// Emitted when the admin updates the liquidity gate config via
/// `set_liquidity_gate_config`.
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
