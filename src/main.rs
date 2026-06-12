//! D2helper overlay entry point.
//!
//! The binary creates a transparent egui window and starts the LoD capture
//! worker immediately. The toolbar can pause or resume snapshot publication
//! while the blocking packet-capture thread keeps waiting for D2GS traffic.

mod app;
mod capture;
mod generated_map;
mod render;
mod snapshot;

use eframe::egui;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::app::D2HelperApp;

fn main() -> eframe::Result<()> {
    let _log_guard = init_logging();

    let viewport = egui::ViewportBuilder::default()
        .with_title("d2helper")
        .with_decorations(true)
        .with_transparent(true)
        .with_inner_size([1120.0, 720.0])
        .with_maximized(true);

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "d2helper",
        options,
        Box::new(|creation_context| Ok(Box::new(D2HelperApp::new(creation_context)))),
    )
}

fn init_logging() -> WorkerGuard {
    let _ = std::fs::create_dir_all("logs");
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open("logs/d2helper.log");
    let file_appender = tracing_appender::rolling::never("logs", "d2helper.log");
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let stdout_layer = fmt::layer().with_writer(std::io::stderr);
    let file_layer = fmt::layer().with_writer(file_writer).with_ansi(false);

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(stdout_layer)
        .with(file_layer)
        .try_init();

    guard
}
