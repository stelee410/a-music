//! Headless engine (SPEC §4): the single authority for `revision`,
//! `transportTick` and the committed score timeline.

pub mod clock;
pub mod schedule;
pub mod state;
pub mod store;
pub mod txn;

pub use schedule::{schedule, NoteEvent, Schedule};
pub use store::ProjectStore;
pub use txn::{apply, plan, set_transport, ApplyOptions, NoRender, Plan, RenderHook};

#[cfg(test)]
mod tests;
