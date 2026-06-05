//! D2helper overlay entry point.
//!
//! The binary creates a transparent egui window and starts with an idle capture
//! worker. Packet capture begins from the toolbar so the UI can be opened before
//! Diablo II has joined a game.

mod app;
mod capture;
mod render;
mod snapshot;

use eframe::egui;
use tracing_subscriber::EnvFilter;

use crate::app::D2HelperApp;

fn main() -> eframe::Result<()> {
    init_logging();

    let viewport = egui::ViewportBuilder::default()
        .with_title("d2helper")
        .with_decorations(true)
        .with_transparent(true)
        .with_inner_size([1120.0, 720.0]);

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

fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}
