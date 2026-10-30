use crate::core::SharedAppState;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

mod contact_book;
pub mod images;
mod messages;
mod presence;
mod session;

pub use messages::{MessageDispatcher, MessagesStream};
pub use presence::{PresenceChangeStream, PresenceHub, UserPresence};

pub async fn gg_server(
  listener: TcpListener,
  shutdown: CancellationToken,
  app_state: SharedAppState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
  tracing::info!(bind = ?listener.local_addr()?, "Listening GG");

  let mut sessions = tokio::task::JoinSet::new();
  loop {
    tokio::select! {
      // Check for a shutdown signal
      _ = shutdown.cancelled() => {
        tracing::info!("GG server shutting down...");
        break;
      }

      Some(result) = sessions.join_next(), if !sessions.is_empty() => {
        if let Err(error) = result { tracing::error!(%error, "Connection task failed"); }
      }
      // Accept new connections
      result = listener.accept() => {
        match result {
          Ok((socket, addr)) => {
            let Some(permit) = app_state.admission.admit(addr.ip()) else { continue; };
            socket.set_nodelay(true)?;
            let conn_shutdown = shutdown.clone();
            let conn_app_state = app_state.clone();
            sessions.spawn(async move {
              let _permit = permit;
              let mut session = session::UserSessionController::new(
                socket,
                conn_shutdown,
                conn_app_state
              );

              let result = async {
                session.establish_session().await?;
                session.sync().await?;
                session.run().await
              }.await;

              if let Err(e) = result {
                tracing::error!("Session failed for {}: {}", addr, e);
              }

              session.cleanup().await;
            });
          },
          Err(e) => {
            tracing::error!("Error accepting connection: {}", e);
            return Err(e.into());
          }
        }
      }
    }
  }

  if tokio::time::timeout(std::time::Duration::from_secs(20), async {
    while sessions.join_next().await.is_some() {}
  })
  .await
  .is_err()
  {
    sessions.abort_all();
  }
  Ok(())
}
