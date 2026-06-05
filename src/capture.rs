//! Blocking D2GS capture worker integration.
//!
//! `libd2r::Client` owns the packet-capture loop and currently runs until the
//! process exits. D2helper starts it on a dedicated thread so the egui event loop
//! can keep repainting while packets are decoded and snapshots are published.

use std::thread;

use libd2r::Client;
use tracing::{error, info, warn};

use crate::snapshot::{
    replace_snapshot, CaptureCounters, CaptureSnapshot, OverlaySnapshot, SharedOverlayState,
};

/// Handle used by the UI thread to start the capture worker once.
pub struct CaptureHandle {
    started: bool,
}

impl CaptureHandle {
    /// Creates a capture handle in the idle state.
    pub fn new() -> Self {
        Self { started: false }
    }

    /// Returns whether the worker has already been started.
    pub fn started(&self) -> bool {
        self.started
    }

    /// Spawns the blocking LoD packet-capture worker.
    pub fn start(&mut self, shared: SharedOverlayState) {
        if self.started {
            warn!("capture worker already started");
            return;
        }
        self.started = true;

        replace_snapshot(
            &shared,
            OverlaySnapshot {
                capture: CaptureSnapshot::starting(),
                ..OverlaySnapshot::default()
            },
        );

        thread::Builder::new()
            .name("d2helper-capture".to_owned())
            .spawn(move || run_capture(shared))
            .expect("failed to spawn d2helper capture thread");
    }
}

impl Default for CaptureHandle {
    fn default() -> Self {
        Self::new()
    }
}

fn run_capture(shared: SharedOverlayState) {
    info!("starting LoD D2GS capture worker");
    let result = std::panic::catch_unwind(move || {
        let mut client = Client::new();
        let mut counters = CaptureCounters::default();

        client.start_with_events(|event, game_state| {
            counters.record(&event);
            let snapshot = OverlaySnapshot::from_game_state(game_state, counters.snapshot(true));
            replace_snapshot(&shared, snapshot);
        });
    });

    if result.is_err() {
        error!("capture worker panicked");
    }
}
