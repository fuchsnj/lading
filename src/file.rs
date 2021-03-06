use crate::config::{self, LogTarget};
use fastrand::Rng;
use governor::state::direct::{self, InsufficientCapacity};
use governor::{clock, state};
use governor::{Quota, RateLimiter};
use metrics::counter;
use std::mem;
use std::num::NonZeroU32;
use std::path::PathBuf;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::io::BufWriter;
use tracing::{debug, info, instrument, span, Level};

#[derive(Debug)]
pub enum Error {
    Governor(InsufficientCapacity),
    Io(::std::io::Error),
    Config(config::Error),
}

impl From<InsufficientCapacity> for Error {
    fn from(error: InsufficientCapacity) -> Self {
        Error::Governor(error)
    }
}

impl From<config::Error> for Error {
    fn from(error: config::Error) -> Self {
        Error::Config(error)
    }
}

impl From<::std::io::Error> for Error {
    fn from(error: ::std::io::Error) -> Self {
        Error::Io(error)
    }
}

#[derive(Debug)]
pub struct Log {
    path: PathBuf,
    fp: BufWriter<fs::File>,
    maximum_bytes_per: NonZeroU32,
    maximum_bytes_burst: NonZeroU32,
    rate_limiter: RateLimiter<direct::NotKeyed, state::InMemoryState, clock::QuantaClock>,
    rng: Rng,
}

impl Log {
    #[instrument]
    pub async fn new(rng: Rng, target: LogTarget) -> Result<Self, Error> {
        let rate_limiter: RateLimiter<direct::NotKeyed, state::InMemoryState, clock::QuantaClock> =
            RateLimiter::direct(
                Quota::per_second(target.bytes_per_second()?)
                    .allow_burst(target.maximum_bytes_burst()?),
            );

        let maximum_bytes_burst = target.maximum_bytes_burst()?;
        let maximum_bytes_per = target.maximum_bytes_per()?;
        let fp = BufWriter::with_capacity(
            maximum_bytes_burst.get() as usize,
            fs::OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(&target.path)
                .await?,
        );

        info!(
            "[{}] maximum_bytes_burst: {}, maximum_bytes_per: {}",
            target.path.to_str().unwrap(),
            maximum_bytes_burst,
            maximum_bytes_per
        );
        Ok(Self {
            fp,
            maximum_bytes_per,
            path: target.path,
            maximum_bytes_burst,
            rate_limiter,
            rng,
        })
    }

    #[instrument]
    #[inline]
    fn fill_buffer(&self, buffer: &mut [u8]) {
        buffer.iter_mut().for_each(|c| *c = self.rng.u8(65..90));
    }

    #[instrument]
    pub async fn spin(mut self) -> Result<(), Error> {
        let mut bytes_written: u64 = 0;
        let maximum_bytes_per: u64 = u64::from(self.maximum_bytes_per.get());
        let maximum_bytes_burst: u32 = self.maximum_bytes_burst.get();

        let mut buffer: Vec<u8> = vec![0; self.maximum_bytes_burst.get() as usize];

        loop {
            let span = span!(Level::INFO, "spin_loop");
            let _enter = span.enter();

            debug!("bytes_written: {}", bytes_written);
            {
                let bytes = self.rng.u32(1..maximum_bytes_burst);
                let nz_bytes = NonZeroU32::new(bytes).unwrap();
                self.rate_limiter.until_n_ready(nz_bytes).await?;

                let slice = &mut buffer[0..bytes as usize];
                self.fill_buffer(slice);
                slice[bytes as usize - 1] = b'\n';

                debug!("writing {} bytes", bytes);
                self.fp.write(slice).await?;
                bytes_written += u64::from(bytes);

                counter!("global_bytes_written", u64::from(bytes));
            }

            if bytes_written > maximum_bytes_per {
                let rot_span = span!(Level::INFO, "rotation");
                let _rot_enter = rot_span.enter();

                info!("rotating file with bytes_written: {}", bytes_written);
                let fp = BufWriter::with_capacity(
                    maximum_bytes_burst as usize,
                    fs::OpenOptions::new()
                        .create(true)
                        .truncate(true)
                        .write(true)
                        .open(&self.path)
                        .await?,
                );
                drop(mem::replace(&mut self.fp, fp));
                bytes_written = 0;
            }
        }
    }
}