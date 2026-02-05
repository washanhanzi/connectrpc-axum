use std::path::PathBuf;
use std::time::Duration;

use tokio::net::{UnixListener, UnixStream};

pub struct TestSocket {
    kind: SocketKind,
}

#[allow(dead_code)] // File variant is unused on Linux, Abstract on non-Linux
enum SocketKind {
    #[cfg(target_os = "linux")]
    Abstract(String),
    File(PathBuf),
}

impl TestSocket {
    pub fn new(name: &str) -> std::io::Result<Self> {
        #[cfg(target_os = "linux")]
        {
            Ok(Self {
                kind: SocketKind::Abstract(name.to_string()),
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let dir = std::env::temp_dir().join("connectrpc-axum-test");
            std::fs::create_dir_all(&dir)?;
            Ok(Self {
                kind: SocketKind::File(dir.join(format!("{name}.sock"))),
            })
        }
    }

    pub fn bind(&self) -> std::io::Result<UnixListener> {
        match &self.kind {
            #[cfg(target_os = "linux")]
            SocketKind::Abstract(name) => {
                use std::os::linux::net::SocketAddrExt;
                use std::os::unix::net::SocketAddr;
                let addr = SocketAddr::from_abstract_name(name)?;
                let listener = std::os::unix::net::UnixListener::bind_addr(&addr)?;
                listener.set_nonblocking(true)?;
                UnixListener::from_std(listener)
            }
            SocketKind::File(path) => {
                let _ = std::fs::remove_file(path);
                UnixListener::bind(path)
            }
        }
    }

    pub async fn connect(&self) -> std::io::Result<UnixStream> {
        match &self.kind {
            #[cfg(target_os = "linux")]
            SocketKind::Abstract(name) => {
                use std::os::linux::net::SocketAddrExt;
                use std::os::unix::net::SocketAddr;
                let name = name.clone();
                let std_stream = tokio::task::spawn_blocking(move || {
                    let addr = SocketAddr::from_abstract_name(&name)?;
                    std::os::unix::net::UnixStream::connect_addr(&addr)
                })
                .await
                .unwrap()?;
                std_stream.set_nonblocking(true)?;
                UnixStream::from_std(std_stream)
            }
            SocketKind::File(path) => UnixStream::connect(path).await,
        }
    }

    /// Returns the socket address for Go processes.
    /// Abstract sockets use `@` prefix, file sockets use the path.
    pub fn go_addr(&self) -> String {
        match &self.kind {
            #[cfg(target_os = "linux")]
            SocketKind::Abstract(name) => format!("@{name}"),
            SocketKind::File(path) => path.to_string_lossy().into_owned(),
        }
    }

    pub async fn wait_ready(&self) -> anyhow::Result<()> {
        for _ in 0..100 {
            if self.connect().await.is_ok() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        anyhow::bail!("socket not ready after 5s")
    }
}

/// Establish an HTTP/2 connection (h2c) over a Unix socket.
/// Returns (sender, connection_join_handle).
pub async fn http2_connect(
    sock: &TestSocket,
) -> anyhow::Result<(
    hyper::client::conn::http2::SendRequest<http_body_util::Full<bytes::Bytes>>,
    tokio::task::JoinHandle<()>,
)> {
    let stream = sock.connect().await?;
    let io = hyper_util::rt::TokioIo::new(stream);

    let (sender, conn) = hyper::client::conn::http2::handshake(
        hyper_util::rt::TokioExecutor::new(),
        io,
    )
    .await?;

    let handle = tokio::spawn(async move {
        if let Err(e) = conn.await {
            eprintln!("h2 connection error: {e}");
        }
    });

    Ok((sender, handle))
}

impl Drop for TestSocket {
    fn drop(&mut self) {
        if let SocketKind::File(path) = &self.kind {
            let _ = std::fs::remove_file(path);
        }
    }
}
