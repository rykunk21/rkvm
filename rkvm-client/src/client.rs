use rkvm_input::writer::Writer;
use rkvm_net::auth::{AuthChallenge, AuthStatus};
use rkvm_net::message::Message;
use rkvm_net::version::Version;
use rkvm_net::{Pong, Update};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::io;
use std::time::Instant;
use thiserror::Error;
use tokio::io::{AsyncWriteExt, BufStream};
use tokio::net::TcpStream;
use tokio::time;
use tokio_rustls::rustls::ServerName;
use tokio_rustls::TlsConnector;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Network error: {0}")]
    Network(io::Error),
    #[error("Input error: {0}")]
    Input(io::Error),
    #[error("Incompatible server version (got {server}, expected {client})")]
    Version { server: Version, client: Version },
    #[error("Invalid password")]
    Auth,
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Input(err)
    }
}

pub async fn run(
    hostname: &ServerName,
    port: u16,
    connector: TlsConnector,
    password: &str,
) -> Result<(), Error> {
    // Initialize RKVM active socket
    let socket_is_active = rkvm_state::init("/run/rkvm/active.sock")
        .await
        .unwrap_or_else(|_| panic!("Failed to initialize active socket"));

    // Connect TCP and TLS ...
    let stream = match hostname {
        ServerName::DnsName(name) => TcpStream::connect(&(name.as_ref(), port)).await,
        ServerName::IpAddress(address) => TcpStream::connect(&(*address, port)).await,
        _ => unimplemented!("Unhandled rustls ServerName variant: {:?}", hostname),
    }
    .map_err(Error::Network)?;
    let stream = rkvm_net::timeout(
        rkvm_net::TLS_TIMEOUT,
        connector.connect(hostname.clone(), stream),
    )
    .await
    .map_err(Error::Network)?;
    let mut stream = BufStream::with_capacity(1024, 1024, stream);

    // Version negotiation and authentication (unchanged) ...
    Version::CURRENT.encode(&mut stream).await?;
    stream.flush().await?;
    let version = Version::decode(&mut stream).await?;
    if version != Version::CURRENT {
        return Err(Error::Version {
            server: Version::CURRENT,
            client: version,
        });
    }
    let challenge = AuthChallenge::decode(&mut stream).await?;
    let response = challenge.respond(password);
    response.encode(&mut stream).await?;
    stream.flush().await?;
    let status = AuthStatus::decode(&mut stream).await?;
    if status != AuthStatus::Passed {
        return Err(Error::Auth);
    }

    tracing::info!("Authenticated successfully");

    let mut writers = HashMap::new();
    let mut start = Instant::now();
    let mut interval = time::interval(rkvm_net::PING_INTERVAL);
    interval.tick().await;

    loop {
        let update = tokio::select! {
            biased;
            update = Update::decode(&mut stream) => update.map_err(Error::Network)?,
            _ = interval.tick() => {
                // Ping timeout
                return Err(Error::Network(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Ping timed out",
                )));
            }
        };

        match update {

            Update::Control { active } => {
                let mut active_socket = socket_is_active.write().await;
                *active_socket = active;
                tracing::info!("RKVM active socket set to: {}", active);
            }

            Update::CreateDevice { id, name, vendor, product, version, rel, abs, keys, delay, period } => {
                let writer: Writer = Writer::builder()?
                    .name(&name)
                    .vendor(vendor)
                    .product(product)
                    .version(version)
                    .rel(rel)?
                    .abs(abs)?
                    .key(keys)?
                    .delay(delay)?
                    .period(period)?
                    .build()
                    .await
                    .map_err(Error::Input)?;

                writers.insert(id, writer);
                tracing::info!(id = %id, "Created new device");
            }

            Update::DestroyDevice { id } => {
                writers.remove(&id);
                tracing::info!(id = %id, "Destroyed device");
            }

            Update::Event { id, event } => {
                let writer = writers.get_mut(&id).ok_or_else(|| {
                    Error::Network(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Server sent an event to a nonexistent device",
                    ))
                })?;
                writer.write(&event).await.map_err(Error::Input)?;
            }

            Update::Ping => {
                let duration = start.elapsed();
                tracing::debug!(duration = ?duration, "Received ping");
                start = Instant::now();
                interval.reset();

                Pong.encode(&mut stream).await?;
                stream.flush().await?;
            }
        }
    }
}