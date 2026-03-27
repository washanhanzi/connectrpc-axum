use redis::AsyncTypedCommands;
use redis::aio::MultiplexedConnection;
use std::sync::atomic::{AtomicUsize, Ordering};

pub const VALKEY_POOL_SIZE: usize = 8;

pub const FORTUNES: &[(i32, &str)] = &[
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
    (12, "Framework Benchmarks"),
];

const KEY: &str = "fortunes";

pub async fn connect(addr: &str) -> redis::RedisResult<MultiplexedConnection> {
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
        let index = self.next.fetch_add(1, Ordering::Relaxed) % self.conns.len();
        self.conns[index].clone()
    }
}

pub async fn seed(conn: &mut MultiplexedConnection) -> redis::RedisResult<()> {
    let mut pipeline = redis::pipe();
    pipeline.del(KEY);
    for &(id, message) in FORTUNES {
        pipeline.hset(KEY, id, message);
    }
    pipeline.query_async(conn).await
}

pub async fn query_fortunes(
    conn: &mut MultiplexedConnection,
) -> redis::RedisResult<Vec<(i32, String)>> {
    let raw = conn.hgetall(KEY).await?;
    let mut fortunes: Vec<(i32, String)> = raw
        .into_iter()
        .map(|(id, message): (String, String)| (id.parse().unwrap_or(0), message))
        .collect();

    fortunes.push((0, "Additional fortune added at request time.".to_string()));
    fortunes.sort_by(|left, right| left.1.cmp(&right.1));
    Ok(fortunes)
}
