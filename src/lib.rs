pub mod ipc;
pub mod session;

/// Subset of the binary's `gui` module that is decoupled from GPUI and
/// can be exercised by integration tests.
pub mod gui {
    pub mod orchestrator_index;
}
