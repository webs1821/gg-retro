mod api;
mod banner;
mod core;
mod downloads;
mod messenger;
mod models;
mod security;

use tokio::net::TcpListener;
use tokio::signal;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  banner::print_banner();

  let subscriber = tracing_subscriber::FmtSubscriber::builder().finish();
  tracing::subscriber::set_global_default(subscriber)?;
  let app_state = core::create_app_state().await?;

  // Create a shutdown token for graceful shutdown coordination
  let shutdown = CancellationToken::new();

  let gg_listener = TcpListener::bind(app_state.config().gg_bind()).await?;
  let web_listener = TcpListener::bind(app_state.config().api_bind()).await?;

  tracing::info!("GG messenger listening on {}", app_state.config().gg_bind());
  tracing::info!(
    "Web API listening on http://{}",
    app_state.config().api_bind()
  );

  // Clone tokens for each task
  let messenger_shutdown = shutdown.clone();
  let api_shutdown = shutdown.clone();

  let messenger_task_app_state = app_state.clone();
  let mut messenger_task = tokio::spawn(async move {
    messenger::gg_server(gg_listener, messenger_shutdown, messenger_task_app_state).await
  });

  let api_task_app_state = app_state.clone();
  let mut api_task =
    tokio::spawn(
      async move { api::http_server(web_listener, api_shutdown, api_task_app_state).await },
    );

  let maintenance_state = app_state.clone();
  let maintenance_shutdown = shutdown.clone();
  let maintenance = tokio::spawn(async move {
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(300));
    loop {
      tokio::select! {
        _ = maintenance_shutdown.cancelled() => break,
        _ = tick.tick() => {
          if let Err(error) = models::MessageRepository::new(maintenance_state.db_pool()).cleanup_old_delivered(60).await {
            tracing::error!(%error, "Queue cleanup failed");
          }
          let _ = sqlx::query("DELETE FROM tokens WHERE used_at IS NOT NULL OR created_at < datetime('now','-5 minutes')").execute(maintenance_state.db_pool()).await;
        }
      }
    }
  });

  // Wait for shutdown signal or task failure
  tokio::select! {
    _ = shutdown_signal() => {
      tracing::info!("Received Ctrl+C, initiating graceful shutdown...");
    }
    result = &mut messenger_task => {
      tracing::error!("GG server exited unexpectedly: {:?}", result);
    }
    result = &mut api_task => {
      tracing::error!("HTTP server exited unexpectedly: {:?}", result);
    }
  }

  // Signal all tasks to shut down
  shutdown.cancel();
  tracing::info!("Shutdown complete");
  if !messenger_task.is_finished() {
    let _ = messenger_task.await;
  }
  if !api_task.is_finished() {
    let _ = api_task.await;
  }
  let _ = maintenance.await;
  Ok(())
}

async fn shutdown_signal() {
  #[cfg(unix)]
  {
    let mut terminate =
      signal::unix::signal(signal::unix::SignalKind::terminate()).expect("SIGTERM handler");
    tokio::select! { _ = signal::ctrl_c() => {}, _ = terminate.recv() => {} }
  }
  #[cfg(not(unix))]
  {
    let _ = signal::ctrl_c().await;
  }
}
