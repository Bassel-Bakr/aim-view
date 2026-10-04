//! Aim View's review server: the UI's server-mode build and the review server's API (python/server.py's, served by
//! the aimview-service crate as the desktop app serves it) over plain HTTP. Settings: config.rs; who gets in:
//! access.rs; requests: http.rs.

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

/// How long requests still open may take to finish after Ctrl+C.
const GRACE: Duration = Duration::from_secs(5);

fn main() -> ExitCode {
    let (settings, file) = match config::load(config::Flags::parse()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("aimview-server: {e}");
            return ExitCode::from(2);
        }
    };
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("aimview-server: {e}");
            return ExitCode::FAILURE;
        }
    };
    let code = runtime.block_on(run(settings, file));
    // a request still in the API's hands is not waited for
    runtime.shutdown_timeout(Duration::from_secs(1));
    code
}

async fn run(settings: config::Settings, file: Option<PathBuf>) -> ExitCode {
    let fail = |e: String| {
        eprintln!("aimview-server: {e}");
        ExitCode::FAILURE
    };
    let host = settings.host.trim_start_matches('[').trim_end_matches(']');
    let addrs: Vec<SocketAddr> = match tokio::net::lookup_host((host, settings.port)).await {
        Ok(a) => a.collect(),
        Err(e) => return fail(format!("the host {}: {e}", settings.host)),
    };
    // one address, IPv4 first: localhost is 127.0.0.1, where the Angular dev server's proxy sends /api
    let Some(addr) = addrs.iter().find(|a| a.is_ipv4()).or(addrs.first()).copied() else {
        return fail(format!("the host {} has no address", settings.host));
    };
    let access = match access::Access::new(&[addr], settings.token.clone()) {
        Ok(a) => a,
        Err(e) => return fail(e),
    };
    if let Some(f) = &file {
        println!("settings: {}", f.display());
    }
    let opened = {
        let settings = settings.clone();
        tokio::task::spawn_blocking(move || glue::open(&settings)).await
    };
    let api = match opened {
        Ok(Ok(api)) => api,
        Ok(Err(e)) => return fail(format!("the library could not open: {e}")),
        Err(e) => return fail(format!("the library could not open: {e}")),
    };
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => return fail(format!("could not listen on {addr}: {e}")),
    };
    println!("data: {}", settings.data.display());
    println!("recordings: {}", settings.vods.as_ref().map_or("the folder chosen in the app".into(), |v| v.display().to_string()));
    println!("stats: {}", settings.stats.display());
    println!("models: {}", settings.models.display());
    println!("{}", glue::describe(api.as_ref(), settings.device));
    if !settings.ui.join("index.html").is_file() {
        println!("no UI build in {} (bun run build:server): only the API answers", settings.ui.display());
    }
    let access_note = if access.has_token() { "with its token" } else { "this machine only" };
    println!("Aim View: {} ({access_note}; Ctrl+C stops it)", settings.url());

    // the recordings and KovaaK's stats folder (tens of thousands of files) are listed before the first page asks, as
    // python/server.py does at its start
    let warm = api.clone();
    tokio::task::spawn_blocking(move || {
        let list = http::Call { method: "GET".into(), path_and_query: "/api/vods".into(), range: None, body: Default::default(), upload: None };
        warm.handle(&list);
    });
    let app = Arc::new(http::App { api, access, ui: settings.ui.clone() });
    let (stop, stopping) = tokio::sync::watch::channel(false);
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_err() {
            return;
        }
        println!("stopping (Ctrl+C again stops at once)");
        let _ = stop.send(true);
        if tokio::signal::ctrl_c().await.is_ok() {
            std::process::exit(130);
        }
    });
    let mut asked = stopping.clone();
    let mut graceful = stopping;
    let serve = axum::serve(listener, http::router(app)).with_graceful_shutdown(async move {
        let _ = graceful.wait_for(|s| *s).await;
    });
    tokio::select! {
        r = serve.into_future() => {
            if let Err(e) = r {
                return fail(format!("the server stopped: {e}"));
            }
        }
        _ = async {
            let _ = asked.wait_for(|s| *s).await;
            tokio::time::sleep(GRACE).await;
        } => println!("requests still open after {} s: stopped anyway", GRACE.as_secs()),
    }
    println!("stopped");
    ExitCode::SUCCESS
}
