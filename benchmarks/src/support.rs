use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use redis::AsyncTypedCommands;
use redis::aio::MultiplexedConnection;

use crate::{Fortune, GetFortunesResponse};

const KEY: &str = "fortunes";
const VALKEY_IMAGE: &str = "valkey/valkey:8-alpine";
const EXTRA_FORTUNE: &str = "Additional fortune added at request time.";
const BENCHMARK_VALKEY_ADDR_ENV: &str = "BENCHMARKS_VALKEY_ADDR";
const LEGACY_FORTUNE_VALKEY_ADDR_ENV: &str = "FORTUNE_VALKEY_ADDR";

pub const STANDARD_FORTUNES: &[(i32, &str)] = &[
    (1, "fortune: No such file or directory"),
    (
        2,
        "A computer scientist is someone who fixes things that aren't broken.",
    ),
    (3, "After enough decimal places, nobody gives a damn."),
    (
        4,
        "A bad random number generator: 1, 1, 1, 1, 1, 4.33e+67, 1, 1, 1",
    ),
    (
        5,
        "A computer program does what you tell it to do, not what you want it to do.",
    ),
    (
        6,
        "Emacs is a nice operating system, but I prefer UNIX. - Tom Christaensen",
    ),
    (7, "Any program that runs right is obsolete."),
    (
        8,
        "A list is only as strong as its weakest link. - Donald Knuth",
    ),
    (9, "Feature: A bug with seniority."),
    (10, "Computers make very fast, very accurate mistakes."),
    (
        11,
        "<script>alert(\"This should not be displayed in a browser alert box.\");</script>",
    ),
    (12, "フレームワークのベンチマーク"),
];

pub async fn connect_valkey(addr: &str) -> redis::RedisResult<MultiplexedConnection> {
    let client = redis::Client::open(format!("redis://{addr}"))?;
    let config = redis::AsyncConnectionConfig::new().set_pipeline_buffer_size(512);
    client
        .get_multiplexed_async_connection_with_config(&config)
        .await
}

pub struct ValkeyPool {
    conns: Vec<MultiplexedConnection>,
    next: AtomicUsize,
}

impl ValkeyPool {
    pub async fn connect(addr: &str, size: usize) -> redis::RedisResult<Self> {
        assert!(size > 0, "ValkeyPool requires at least one connection");

        let client = redis::Client::open(format!("redis://{addr}"))?;
        let config = redis::AsyncConnectionConfig::new().set_pipeline_buffer_size(512);
        let mut conns = Vec::with_capacity(size);
        for _ in 0..size {
            conns.push(
                client
                    .get_multiplexed_async_connection_with_config(&config)
                    .await?,
            );
        }

        Ok(Self {
            conns,
            next: AtomicUsize::new(0),
        })
    }

    pub fn get(&self) -> MultiplexedConnection {
        let idx = self.next.fetch_add(1, Ordering::Relaxed) % self.conns.len();
        self.conns[idx].clone()
    }
}

pub async fn seed_fortunes(conn: &mut MultiplexedConnection) -> redis::RedisResult<()> {
    let mut pipe = redis::pipe();
    pipe.del(KEY);
    for &(id, message) in STANDARD_FORTUNES {
        pipe.hset(KEY, id, message);
    }
    pipe.query_async(conn).await
}

pub async fn query_fortunes(
    conn: &mut MultiplexedConnection,
) -> redis::RedisResult<Vec<(i32, String)>> {
    let raw = conn.hgetall(KEY).await?;
    let mut fortunes: Vec<(i32, String)> = raw
        .into_iter()
        .map(|(id, message): (String, String)| (id.parse().unwrap_or_default(), message))
        .collect();
    fortunes.push((0, EXTRA_FORTUNE.to_string()));
    fortunes.sort_by(|left, right| left.1.cmp(&right.1));
    Ok(fortunes)
}

pub async fn load_response(pool: &ValkeyPool) -> Result<GetFortunesResponse> {
    let mut conn = pool.get();
    let fortunes = query_fortunes(&mut conn)
        .await
        .context("querying fortunes from Valkey")?;

    Ok(GetFortunesResponse {
        fortunes: fortunes
            .into_iter()
            .map(|(id, message)| Fortune { id, message })
            .collect(),
    })
}

pub struct ValkeyContainer {
    addr: String,
    name: Option<String>,
}

impl ValkeyContainer {
    pub async fn start() -> Result<Self> {
        if let Some(addr) = std::env::var(BENCHMARK_VALKEY_ADDR_ENV)
            .ok()
            .or_else(|| std::env::var(LEGACY_FORTUNE_VALKEY_ADDR_ENV).ok())
        {
            let container = Self { addr, name: None };
            container.seed().await?;
            return Ok(container);
        }

        let name = format!("connectrpc-axum-bench-valkey-{}", std::process::id());
        let output = Command::new("docker")
            .args([
                "run",
                "-d",
                "--rm",
                "-p",
                "127.0.0.1::6379",
                "--name",
                &name,
                VALKEY_IMAGE,
            ])
            .output()
            .context("starting Valkey container")?;

        if !output.status.success() {
            bail!(
                "docker run failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        let port_output = Command::new("docker")
            .args(["port", &name, "6379"])
            .output()
            .context("querying Valkey port")?;

        if !port_output.status.success() {
            bail!(
                "docker port failed: {}",
                String::from_utf8_lossy(&port_output.stderr).trim()
            );
        }

        let addr = String::from_utf8_lossy(&port_output.stdout)
            .lines()
            .next()
            .context("docker port returned no mappings")?
            .trim()
            .to_string();

        let container = Self {
            addr,
            name: Some(name),
        };
        container.seed().await?;
        Ok(container)
    }

    pub fn addr(&self) -> &str {
        &self.addr
    }

    async fn seed(&self) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut conn = loop {
            match connect_valkey(&self.addr).await {
                Ok(conn) => break conn,
                Err(_) if Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Err(err) => {
                    bail!(
                        "Valkey at {} was not ready within 5 seconds: {err}",
                        self.addr
                    );
                }
            }
        };

        seed_fortunes(&mut conn)
            .await
            .context("seeding Fortune benchmark data")?;
        Ok(())
    }
}

impl Drop for ValkeyContainer {
    fn drop(&mut self) {
        if let Some(name) = &self.name {
            let _ = Command::new("docker").args(["rm", "-f", name]).output();
        }
    }
}
