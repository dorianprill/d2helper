//! Blocking D2GS capture worker integration.
//!
//! `libd2r::Client` owns the packet-capture loop and currently runs until the
//! process exits. D2helper starts it on a dedicated thread so the egui event loop
//! can keep repainting while packets are decoded and snapshots are published.

use std::{any::Any, thread};

use libd2r::{Client, ConnectionEvent, ServerMessageParseError};
use pnet::datalink::{self, NetworkInterface};
use tracing::{error, info, warn};

use crate::snapshot::{
    replace_capture, replace_snapshot, CaptureCounters, CaptureSnapshot, OverlaySnapshot,
    SharedOverlayState,
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
    log_process_capabilities();
    log_capture_interfaces();
    replace_capture(&shared, CaptureSnapshot::waiting());

    let worker_shared = shared.clone();
    let result = std::panic::catch_unwind(move || {
        let mut client = Client::new();
        let mut counters = CaptureCounters::default();

        client.start_with_events(|event, game_state| {
            log_connection_event(&event);
            counters.record(&event);
            let snapshot = OverlaySnapshot::from_game_state(game_state, counters.snapshot(true));
            replace_snapshot(&worker_shared, snapshot);
        });
    });

    if let Err(payload) = result {
        let error = panic_payload_label(payload.as_ref());
        error!(%error, "capture worker panicked");
        replace_capture(&shared, CaptureSnapshot::failed(error));
    }
}

fn log_connection_event(event: &ConnectionEvent) {
    match event {
        ConnectionEvent::ServerMessage {
            packet, applied, ..
        } => {
            info!(
                packet_id = %format_args!("0x{:02X}", packet.packet_id()),
                len = packet.data.len(),
                applied,
                "parsed D2GS server packet"
            );
        }
        ConnectionEvent::ParseError { packet, error } => match error {
            ServerMessageParseError::UnexpectedLength {
                expected, actual, ..
            } => {
                warn!(
                    packet_id = %format_args!("0x{:02X}", packet.packet_id()),
                    expected,
                    actual,
                    len = packet.data.len(),
                    bytes = %hex_prefix(&packet.data, 96),
                    "D2GS server packet length mismatch"
                );
            }
            ServerMessageParseError::UnsupportedPacketId(packet_id) => {
                warn!(
                    packet_id = %format_args!("0x{packet_id:02X}"),
                    len = packet.data.len(),
                    bytes = %hex_prefix(&packet.data, 96),
                    "unsupported D2GS server packet"
                );
            }
            ServerMessageParseError::EmptyPacket => {
                warn!("empty D2GS server packet");
            }
        },
    }
}

fn hex_prefix(bytes: &[u8], limit: usize) -> String {
    let mut rendered = bytes
        .iter()
        .take(limit)
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ");
    if bytes.len() > limit {
        rendered.push_str(" ...");
    }
    rendered
}

fn panic_payload_label(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else if let Some(message) = payload.downcast_ref::<&'static str>() {
        (*message).to_owned()
    } else {
        "capture worker panicked with non-string payload".to_owned()
    }
}

fn log_capture_interfaces() {
    let interfaces = datalink::interfaces();
    info!(
        count = interfaces.len(),
        "available packet-capture interfaces"
    );
    for interface in &interfaces {
        info!(
            name = %interface.name,
            description = %interface.description,
            index = interface.index,
            up = interface.is_up(),
            loopback = interface.is_loopback(),
            point_to_point = interface.is_point_to_point(),
            ips = ?interface.ips,
            "packet-capture interface"
        );
    }

    if let Some(candidate) = interfaces
        .iter()
        .find(|interface| libd2_candidate(interface))
    {
        info!(
            name = %candidate.name,
            index = candidate.index,
            ips = ?candidate.ips,
            "first libd2-style capture interface candidate"
        );
    } else {
        warn!("no libd2-style capture interface candidate found");
    }
}

#[cfg(not(target_os = "windows"))]
fn libd2_candidate(interface: &NetworkInterface) -> bool {
    interface.is_up() && !interface.is_loopback() && !interface.ips.is_empty()
}

#[cfg(target_os = "linux")]
fn log_process_capabilities() {
    match std::fs::read_to_string("/proc/self/status") {
        Ok(status) => {
            for line in status.lines().filter(|line| {
                line.starts_with("CapInh:")
                    || line.starts_with("CapPrm:")
                    || line.starts_with("CapEff:")
                    || line.starts_with("CapBnd:")
                    || line.starts_with("CapAmb:")
                    || line.starts_with("NoNewPrivs:")
            }) {
                info!(%line, "process capability status");
            }
        }
        Err(error) => warn!(%error, "failed to read /proc/self/status"),
    }
}

#[cfg(not(target_os = "linux"))]
fn log_process_capabilities() {}

#[cfg(target_os = "windows")]
fn libd2_candidate(interface: &NetworkInterface) -> bool {
    use pnet::ipnetwork::IpNetwork;
    use std::net::{IpAddr, Ipv4Addr};

    interface
        .ips
        .first()
        .is_some_and(|ip| *ip != IpNetwork::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0).unwrap())
}
