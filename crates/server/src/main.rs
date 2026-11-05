//! Entry point of the Schisaciaccole server.

use anyhow::Context;
use schisaciaccole_core::api::AUTH_CALLBACK_PATH;
use schisaciaccole_server::app::{AppState, router};
use schisaciaccole_server::config::Config;
use schisaciaccole_server::db;
use schisaciaccole_server::google::GoogleOidc;
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    init_tracing();

    let config = Config::from_env().context("invalid configuration")?;
    let bind_addr = config.bind_addr;
    let pool = db::connect(&config.database_url)
        .await
        .context("database connection failed")?;
    db::migrate(&pool)
        .await
        .context("database migration failed")?;
    let google = GoogleOidc::discover(&config, AUTH_CALLBACK_PATH)
        .await
        .with_context(|| {
            format!(
                "Google OpenID discovery failed for {}",
                config.google_issuer_url
            )
        })?;
    let app = router(AppState::new(config, pool, google));

    let listener = TcpListener::bind(bind_addr)
        .await
        .with_context(|| format!("cannot bind {bind_addr}"))?;
    info!(%bind_addr, "server listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")?;
    info!("server stopped");
    Ok(())
}

/// Initializes logging from `RUST_LOG`, defaulting to `info`.
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

/// Resolves when Ctrl-C or SIGTERM is received.
///
/// If a signal handler cannot be installed, that signal is ignored and the
/// other one still triggers the shutdown.
async fn shutdown_signal() {
    let ctrl_c = async {
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}
