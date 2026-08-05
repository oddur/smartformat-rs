//! A differential fuzzer for the SmartFormat port.
//!
//! It generates plausible templates from a grammar ([`gen`]), renders each one
//! with the real SmartFormat.NET through `tools/goldens` ([`dotnetside`]) and
//! with this port through the same mapping the golden runner uses
//! ([`rustside`]), diffs the two byte for byte, sorts the differences into
//! "something `DESIGN.md` already covers" and "new" ([`classify`]), and shrinks
//! the new ones until they fit on a line ([`shrink`]).
//!
//! The crate is a library as well as a binary so its own tests can drive the
//! whole pipeline against the stand-in harness in `src/bin/fake_harness.rs`,
//! including the parts — a batch that kills the harness, a batch that hangs —
//! that the real one only reaches by accident.
//!
//! See `tools/difffuzz/README.md` for how to run a campaign and how to read
//! what comes out.

pub mod campaign;
pub mod case;
pub mod classify;
pub mod dotnetside;
pub mod gen;
pub mod report;
pub mod rng;
pub mod rustside;
pub mod shrink;
