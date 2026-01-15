use std::fs;
use std::path::Path;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::RwLock;
use tokio::time::{self, Duration};

pub type SocketState = Arc<RwLock<bool>>;

/// Initialize the UNIX socket at `socket_path` and spawn broadcaster task.
/// Returns a handle to the active state (true = active, false = inactive).
pub async fn init(socket_path: &str) -> std::io::Result<SocketState> {
    // Ensure parent directory exists
    if let Some(parent) = Path::new(socket_path).parent() {
        fs::create_dir_all(parent)?;
    }

    // Remove stale socket
    let _ = fs::remove_file(socket_path);

    let listener = UnixListener::bind(socket_path)?;
    let active = Arc::new(RwLock::new(true));
    let active_clone = active.clone();

    // Spawn broadcaster task
    tokio::spawn(async move {
        let mut clients: Vec<UnixStream> = Vec::new();

        loop {
            tokio::select! {
                Ok((stream, _)) = listener.accept() => {
                    clients.push(stream);
                }
                _ = time::sleep(Duration::from_millis(100)) => {
                    let val = *active_clone.read().await;
                    let msg = format!("active_rkvm={}\n", val);

                    let mut alive = Vec::new();
                    for mut c in clients.drain(..) {
                        if c.write_all(msg.as_bytes()).await.is_ok() {
                            alive.push(c);
                        }
                    }
                    clients = alive;
                }
            }
        }
    });

    Ok(active)
}

pub async fn set_active(state: &SocketState) {
    let mut write = state.write().await;
    *write = true;
}

pub async fn set_inactive(state: &SocketState) {
    let mut write = state.write().await;
    *write = false;
}
