//! Aim View's review server: the UI's server-mode build and the review server's API (python/retired/server.py's,
//! served by the aimview-service crate as the desktop app serves it) over plain HTTP.
//!
//! In: the command line and the settings file (config.rs). Out: the answers on the address it listens on, until
//! Ctrl+C, and a log of its settings and requests on standard output. Who gets in: access.rs; requests: http.rs; the
//! UI's files: files.rs; the review service: glue.rs.

mod access;
mod config;
mod files;
mod glue;
mod http;

use std::future::IntoFuture;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use tokio::net::TcpListener;
use tokio::sync::watch;

use crate::config::Settings;
use crate::http::Api;

/// How long requests still open may take to finish after Ctrl+C.
const STOP_GRACE: Duration = Duration::from_secs(5);
/// How long the runtime waits for its threads once the server stopped: a request still in the API's hands is not
/// waited for.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);
/// The exit code when the settings cannot be read (clap's for bad flags).
const EXIT_BAD_SETTINGS: u8 = 2;
/// The exit code for a second Ctrl+C: 128 + SIGINT, as a shell gives a program it interrupts.
const EXIT_INTERRUPTED: i32 = 130;

/// Reads the settings, serves on a Tokio runtime until Ctrl+C, and exits with 0, 1 on an error, or 2 when the
/// settings cannot be read.
fn main() -> ExitCode {
    let (settings, file) = match config::load(config::Flags::parse()) {
        Ok(loaded) => loaded,
        Err(message) => {
            eprintln!("aimview-server: {message}");
            return ExitCode::from(EXIT_BAD_SETTINGS);
        }
    };
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("aimview-server: {error}");
            return ExitCode::FAILURE;
        }
    };
    let code = match runtime.block_on(run(settings, file)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("aimview-server: {message}");
            ExitCode::FAILURE
        }
    };
    runtime.shutdown_timeout(SHUTDOWN_TIMEOUT);
    code
}

/// Opens the library and serves until Ctrl+C. `file` is the settings file read, if any; the error is the message
/// to print.
async fn run(settings: Settings, file: Option<PathBuf>) -> Result<(), String> {
    let address = listen_address(&settings).await?;
    let access = access::Access::new(&[address], settings.token.clone(), settings.dev)?;
    if let Some(path) = &file {
        println!("settings: {}", path.display());
    }
    let api = open_library(&settings).await?;
    let listener =
        TcpListener::bind(address).await.map_err(|error| format!("could not listen on {address}: {error}"))?;
    print_settings(&settings, api.as_ref(), &access);
    list_recordings_early(api.clone());
    serve(listener, Arc::new(http::App { api, access, ui: settings.ui.clone() })).await
}

/// The one address to listen on, IPv4 first: localhost is 127.0.0.1, where the Angular dev server's proxy sends /api.
async fn listen_address(settings: &Settings) -> Result<SocketAddr, String> {
    let host = settings.host.trim_start_matches('[').trim_end_matches(']');
    let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host, settings.port))
        .await
        .map_err(|error| format!("the host {}: {error}", settings.host))?
        .collect();
    addresses
        .iter()
        .find(|address| address.is_ipv4())
        .or(addresses.first())
        .copied()
        .ok_or_else(|| format!("the host {} has no address", settings.host))
}

/// The library the settings describe, opened on a blocking thread (it reads the data folder).
async fn open_library(settings: &Settings) -> Result<Arc<dyn Api>, String> {
    let settings = settings.clone();
    match tokio::task::spawn_blocking(move || glue::open(&settings)).await {
        Ok(Ok(api)) => Ok(api),
        Ok(Err(error)) => Err(format!("the library could not open: {error}")),
        Err(error) => Err(format!("the library could not open: {error}")),
    }
}

/// The log's first lines: the folders, the detector and the address to open.
fn print_settings(settings: &Settings, api: &dyn Api, access: &access::Access) {
    println!("data: {}", settings.data.display());
    let recordings = settings
        .vods
        .as_ref()
        .map_or_else(|| "the folder chosen in the app".to_string(), |folder| folder.display().to_string());
    println!("recordings: {recordings}");
    println!("stats: {}", settings.stats.display());
    println!("models: {}", settings.models.display());
    println!("{}", glue::describe(api, settings.device));
    if !settings.ui.join("index.html").is_file() {
        println!("no UI build in {} (bun run build:server): only the API answers", settings.ui.display());
    }
    let access_note = if access.is_open_network() {
        "dev mode: open to the local network without a token"
    } else if access.has_token() {
        "with its token"
    } else {
        "this machine only"
    };
    if settings.dev && settings.token.is_some() {
        println!("dev mode: the token is not used");
    }
    println!("Aim View: {} ({access_note}; Ctrl+C stops it)", settings.url());
}

/// Lists the recordings and KovaaK's stats folder (tens of thousands of files) before the first page asks, as
/// python/retired/server.py did at its start.
fn list_recordings_early(api: Arc<dyn Api>) {
    tokio::task::spawn_blocking(move || {
        api.handle(&http::Call::get("/api/vods"));
    });
}

/// Serves until Ctrl+C, then gives the requests still open STOP_GRACE to finish.
async fn serve(listener: TcpListener, app: Arc<http::App>) -> Result<(), String> {
    let (stop, stopping) = watch::channel(false);
    tokio::spawn(stop_on_ctrl_c(stop));
    let mut asked = stopping.clone();
    let mut graceful = stopping;
    let server = axum::serve(listener, http::router(app)).with_graceful_shutdown(async move {
        let _ = graceful.wait_for(|stopped| *stopped).await;
    });
    tokio::select! {
        served = server.into_future() => {
            if let Err(error) = served {
                return Err(format!("the server stopped: {error}"));
            }
        }
        _ = async {
            let _ = asked.wait_for(|stopped| *stopped).await;
            tokio::time::sleep(STOP_GRACE).await;
        } => println!("requests still open after {} s: stopped anyway", STOP_GRACE.as_secs()),
    }
    println!("stopped");
    Ok(())
}

/// Sends `stop` at the first Ctrl+C; the second one exits at once.
async fn stop_on_ctrl_c(stop: watch::Sender<bool>) {
    if tokio::signal::ctrl_c().await.is_err() {
        return;
    }
    println!("stopping (Ctrl+C again stops at once)");
    let _ = stop.send(true);
    if tokio::signal::ctrl_c().await.is_ok() {
        std::process::exit(EXIT_INTERRUPTED);
    }
}
