//! Blocking D2GS capture worker integration.
//!
//! `libd2r::Client` owns the packet-capture loop and currently runs until the
//! process exits. D2helper starts it on a dedicated thread so the egui event loop
//! can keep repainting while packets are decoded and snapshots are published.

use std::{
    any::Any,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
};

use libd2r::{
    Client, ConnectionEvent, ConnectionTransportWarning, GameData, ServerMessageParseError,
};
use pnet::datalink::{self, NetworkInterface};
use tracing::{error, info, warn};

use crate::generated_map::GeneratedMapCache;
use crate::snapshot::{
    replace_capture, replace_snapshot, CaptureCounters, CaptureSnapshot, OverlaySnapshot,
    SharedOverlayState,
};

/// Handle used by the UI thread to start the capture worker once.
pub struct CaptureHandle {
    started: bool,
    enabled: Arc<AtomicBool>,
}

impl CaptureHandle {
    /// Creates a capture handle in the idle state.
    pub fn new() -> Self {
        Self {
            started: false,
            enabled: Arc::new(AtomicBool::new(true)),
        }
    }

    /// Returns whether decoded traffic is currently published to the UI.
    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Toggles snapshot publication while leaving the blocking worker alive.
    ///
    /// `libd2r::Client` currently owns a blocking raw-channel listener. Keeping
    /// that worker alive avoids platform-specific cancellation of a packet
    /// capture channel and lets the UI resume on the next decoded D2GS event.
    pub fn toggle_enabled(&self, shared: &SharedOverlayState) {
        let enabled = !self.enabled();
        self.enabled.store(enabled, Ordering::Relaxed);
        if let Ok(mut guard) = shared.write() {
            guard.capture.running = self.started && enabled;
            guard.capture.status = if enabled {
                "waiting for LoD D2GS traffic on TCP port 4000".to_owned()
            } else {
                "capture stopped".to_owned()
            };
        }
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

        let enabled = self.enabled.clone();
        thread::Builder::new()
            .name("d2helper-capture".to_owned())
            .spawn(move || run_capture(shared, enabled))
            .expect("failed to spawn d2helper capture thread");
    }
}

impl Default for CaptureHandle {
    fn default() -> Self {
        Self::new()
    }
}

fn run_capture(shared: SharedOverlayState, enabled: Arc<AtomicBool>) {
    info!("starting LoD D2GS capture worker");
    log_process_capabilities();
    log_capture_interfaces();
    replace_capture(&shared, CaptureSnapshot::waiting());

    let worker_shared = shared.clone();
    let result = std::panic::catch_unwind(move || {
        let mut client = Client::new();
        let mut counters = CaptureCounters::default();
        let game_data = load_static_game_data();
        let mut generated_maps = GeneratedMapCache::from_env();

        client.start_with_events(|event, game_state| {
            log_connection_event(&event);
            counters.record(&event);
            if !enabled.load(Ordering::Relaxed) {
                replace_capture(&worker_shared, counters.snapshot(false));
                return;
            }
            let generated_map = generated_maps.current_map(game_state);
            let snapshot = OverlaySnapshot::from_game_state_with_data_and_map(
                game_state,
                counters.snapshot(true),
                game_data.as_deref(),
                generated_map,
            );
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
        ConnectionEvent::TransportWarning { warning } => log_transport_warning(warning),
    }
}

fn log_transport_warning(warning: &ConnectionTransportWarning) {
    match warning {
        ConnectionTransportWarning::DuplicateTcpSegment {
            sequence,
            len,
            expected_sequence,
        } => {
            info!(
                sequence,
                len, expected_sequence, "ignored duplicate D2GS TCP segment"
            );
        }
        ConnectionTransportWarning::OverlappingTcpSegment {
            sequence,
            skipped,
            emitted,
            expected_sequence,
        } => {
            info!(
                sequence,
                skipped, emitted, expected_sequence, "trimmed overlapping D2GS TCP segment"
            );
        }
        ConnectionTransportWarning::OutOfOrderTcpSegment {
            sequence,
            len,
            expected_sequence,
            buffered_segments,
            buffered_bytes,
        } => {
            warn!(
                sequence,
                len,
                expected_sequence,
                buffered_segments,
                buffered_bytes,
                "buffered out-of-order D2GS TCP segment"
            );
        }
        ConnectionTransportWarning::BufferedTcpSegmentReleased { sequence, len } => {
            info!(
                sequence,
                len, "released buffered D2GS TCP segment after gap filled"
            );
        }
        ConnectionTransportWarning::TcpGapReset {
            sequence,
            len,
            expected_sequence,
            buffered_segments,
            buffered_bytes,
        } => {
            warn!(
                sequence,
                len,
                expected_sequence,
                buffered_segments,
                buffered_bytes,
                "reset D2GS reader after missing TCP gap exceeded buffer limit"
            );
        }
        ConnectionTransportWarning::BufferedD2gsPayload {
            payload_len,
            buffered_len,
        } => {
            info!(
                payload_len,
                buffered_len, "buffered partial D2GS payload waiting for more TCP bytes"
            );
        }
        ConnectionTransportWarning::D2gsFramingReset {
            payload_len,
            discarded_len,
        } => {
            warn!(
                payload_len,
                discarded_len, "reset D2GS packet framing after buffered payload exceeded limit"
            );
        }
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

fn load_static_game_data() -> Option<Arc<GameData>> {
    let Some(path) = static_game_data_path() else {
        warn!(
            "no Classic/LoD install path found for MPQ static data; object, item, and monster names stay raw"
        );
        return None;
    };

    match GameData::load_classic_lod_install(&path) {
        Ok(data) => {
            info!(
                path = %path.display(),
                monsters = data.monster_count(),
                objects = data.object_count(),
                levels = data.level_count(),
                items = data.item_count(),
                "loaded Classic/LoD MPQ static data"
            );
            Some(Arc::new(data))
        }
        Err(error) => {
            warn!(
                path = %path.display(),
                %error,
                "failed to load Classic/LoD MPQ static data"
            );
            None
        }
    }
}

fn static_game_data_path() -> Option<PathBuf> {
    for var in ["D2HELPER_D2_PATH", "LIBD2_D2_INSTALL"] {
        if let Some(path) = std::env::var_os(var).map(PathBuf::from) {
            if path_contains_legacy_mpqs(&path) {
                return Some(path);
            }
            warn!(
                var,
                path = %path.display(),
                "configured Diablo II path does not contain legacy MPQs"
            );
        }
    }

    let games = PathBuf::from(std::env::var_os("HOME")?).join("Games");
    let entries = std::fs::read_dir(games).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.starts_with("Diablo II") && path_contains_legacy_mpqs(&path) {
            return Some(path);
        }
    }
    None
}

fn path_contains_legacy_mpqs(path: &Path) -> bool {
    path.join("patch_d2.mpq").exists() && path.join("d2data.mpq").exists()
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
