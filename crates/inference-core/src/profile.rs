//! Energy-profile selection policy.
//!
//! Consumes [`resource_monitor::Signals`] and decides the active
//! [`ProfileName`] (PowerSaver / Balanced / Performance) via additive
//! scoring, hysteresis, an Auto/PinnedSoft override FSM, and emergency
//! triggers. A profile maps to a [`ProfilePolicy`] (resource knobs:
//! idle-unload timeouts, thread count, learning pace), consumed by the
//! M5.3 Engine — not yet wired.
//!
//! The decision logic is split into pure free functions + a [`Decider`]
//! that takes an injected `Instant` (deterministically testable) and a
//! thin async [`ProfileSelector`] shell that consumes a broadcast of
//! `Signals`.
