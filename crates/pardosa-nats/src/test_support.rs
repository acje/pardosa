//! Live NATS test harness and server manager.

use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};
use tempfile::TempDir;

const READINESS_BUDGET: Duration = Duration::from_secs(10);
const READINESS_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Managed local NATS server instance running with JetStream enabled.
pub struct LiveNatsServer {
    url: String,
    _tempdir: TempDir,
    child: Mutex<Option<Child>>,
}

impl LiveNatsServer {
    /// Returns a shared handle to a running JetStream-enabled local `nats-server`.
    ///
    /// # Panics
    /// Panics if `nats-server` cannot be spawned or fails to become ready.
    #[must_use]
    pub fn acquire() -> Arc<Self> {
        static SINGLETON: OnceLock<Mutex<Weak<LiveNatsServer>>> = OnceLock::new();
        let cell = SINGLETON.get_or_init(|| Mutex::new(Weak::new()));
        let mut guard = cell
            .lock()
            .expect("LiveNatsServer singleton mutex poisoned");
        if let Some(existing) = guard.upgrade() {
            return existing;
        }
        let fresh = Arc::new(Self::spawn(None).expect("failed to spawn live nats-server"));
        *guard = Arc::downgrade(&fresh);
        fresh
    }

    /// Starts a private server whose lifetime is independent of the shared fixture.
    ///
    /// # Panics
    /// Panics if the private server cannot start.
    #[must_use]
    pub fn isolated() -> Self {
        Self::spawn(None).expect("failed to spawn isolated nats-server")
    }

    /// Starts a private server with a custom configuration file whose lifetime is independent of
    /// the shared fixture.
    ///
    /// The configuration is written to a temp directory and passed via `-c`. A literal
    /// `{store_dir}` placeholder is replaced with an absolute temporary directory so JetStream
    /// storage stays per-server and never pollutes the repository.
    ///
    /// # Panics
    /// Panics if the private server cannot start.
    #[must_use]
    pub fn isolated_with_config(config: &str) -> Self {
        Self::spawn(Some(config)).expect("failed to spawn configured nats-server")
    }

    /// URL of the running server in `nats://127.0.0.1:<port>` format.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    fn spawn(config: Option<&str>) -> Result<Self, std::io::Error> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        drop(listener);

        let tempdir = TempDir::new()?;
        let host = "127.0.0.1";
        let mut cmd = Command::new("nats-server");
        cmd.arg("-a").arg(host).arg("-p").arg(port.to_string());
        match config {
            None => {
                cmd.arg("-js").arg("-sd").arg(tempdir.path());
            }
            Some(config_body) => {
                let config_path = tempdir.path().join("nats-server.conf");
                let resolved =
                    config_body.replace("{store_dir}", &tempdir.path().display().to_string());
                std::fs::write(&config_path, resolved)?;
                cmd.arg("-c").arg(&config_path);
            }
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = cmd.spawn()?;

        let url = format!("nats://{host}:{port}");
        if let Err(err) = wait_for_readiness(&url) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(err);
        }

        Ok(Self {
            url,
            _tempdir: tempdir,
            child: Mutex::new(Some(child)),
        })
    }
}

fn wait_for_readiness(url: &str) -> Result<(), std::io::Error> {
    let host_port = url.strip_prefix("nats://").unwrap_or(url);
    let start = Instant::now();
    while start.elapsed() < READINESS_BUDGET {
        if std::net::TcpStream::connect(host_port).is_ok() {
            return Ok(());
        }
        std::thread::sleep(READINESS_POLL_INTERVAL);
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        format!("timed out waiting for nats-server readiness at {url}"),
    ))
}

impl Drop for LiveNatsServer {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.child.lock() {
            if let Some(mut child) = guard.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}
