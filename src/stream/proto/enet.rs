use std::{
    convert::Infallible,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use rusty_enet::{Event, Host, HostSettings, Peer, PeerID, ReadWrite};
use thiserror::Error;
use tracing::{debug, trace};

#[derive(Debug, Error)]
pub enum EnetError {
    #[error("bad enet parameter: {0}")]
    BadParameter(#[from] rusty_enet::error::BadParameter),
    #[error("no available peers: {0}")]
    NoAvailablePeers(#[from] rusty_enet::error::NoAvailablePeers),
    #[error("no available peers: {0}")]
    PeerSendError(#[from] rusty_enet::error::PeerSendError),
}

impl From<rusty_enet::error::HostNewError<ReadWrite<SocketAddr, Infallible>>> for EnetError {
    fn from(value: rusty_enet::error::HostNewError<ReadWrite<SocketAddr, Infallible>>) -> Self {
        match value {
            rusty_enet::error::HostNewError::BadParameter(parameter) => {
                Self::BadParameter(parameter)
            }
            rusty_enet::error::HostNewError::FailedToInitializeSocket(_) => unreachable!(),
        }
    }
}

pub struct EnetConfig {
    pub peer_count: usize,
    pub channel_limit: usize,
    pub incoming_bandwidth: Option<usize>,
    pub outgoing_bandwidth: Option<usize>,
}

pub enum EnetInput<'a> {
    Receive {
        now: Instant,
        addr: SocketAddr,
        data: &'a [u8],
    },
    Timeout(Instant),
}
#[derive(Debug)]
pub enum EnetOutput {
    Send { addr: SocketAddr, data: Vec<u8> },
    Event(EnetEvent),
    Timeout(Instant),
}

#[derive(Debug)]
pub enum EnetEvent {
    Connect {
        peer: PeerID,
        data: u32,
    },
    Receive {
        peer: PeerID,
        channel_id: u8,
        data: Vec<u8>,
    },
    Disconnect {
        peer: PeerID,
        data: u32,
    },
}

pub struct EnetHost {
    last_now: Arc<Mutex<Instant>>,
    enet: Host<ReadWrite<SocketAddr, Infallible>>,
}

impl EnetHost {
    pub fn new(now: Instant, config: EnetConfig) -> Self {
        let last_now = Arc::new(Mutex::new(now));

        // This unwrap is safe because those settings don't fail, which would be the only error source
        #[allow(clippy::unwrap_used)]
        let enet = Host::new(
            ReadWrite::<SocketAddr, Infallible>::new(),
            HostSettings {
                peer_limit: config.peer_count,
                channel_limit: config.channel_limit,
                incoming_bandwidth_limit: config.incoming_bandwidth.map(|x| x as u32),
                outgoing_bandwidth_limit: config.outgoing_bandwidth.map(|x| x as u32),
                time: {
                    let start = now;
                    let last_now = last_now.clone();
                    Box::new(move || {
                        let last_now = {
                            // This is allowed because we:
                            // 1. only we have got access to this mutex
                            // 2. don't panic inside this implementation
                            #[allow(clippy::unwrap_used)]
                            let last_now = last_now.lock().unwrap();

                            *last_now
                        };

                        last_now - start
                    })
                },
                ..Default::default()
            },
        )
        .unwrap();

        Self { last_now, enet }
    }

    pub fn connect(
        &mut self,
        addr: SocketAddr,
        channel_count: usize,
        data: u32,
    ) -> Result<PeerID, EnetError> {
        debug!(remote_addr = ?addr, connect_data = ?data, "Enet starting connect");

        let peer = self.enet.connect(addr, channel_count, data)?;

        Ok(peer.id())
    }

    pub fn peer(&mut self, id: PeerID) -> Option<&mut Peer<ReadWrite<SocketAddr, Infallible>>> {
        self.enet.get_peer_mut(id)
    }

    pub fn poll_output(&mut self) -> Result<EnetOutput, EnetError> {
        if let Some((addr, data)) = self.enet.socket_mut().read() {
            return Ok(EnetOutput::Send { addr, data });
        }

        // The error is infallible and cannot be constructed
        // -> we are allowed to unwrap
        #[allow(clippy::unwrap_used)]
        if let Some(event) = self.enet.service().unwrap() {
            match event {
                Event::Connect { peer, data } => {
                    debug!(peer_id = ?peer, connect_data = ?data, "Enet connected");

                    return Ok(EnetOutput::Event(EnetEvent::Connect {
                        peer: peer.id(),
                        data,
                    }));
                }
                Event::Receive {
                    peer,
                    channel_id,
                    packet,
                } => {
                    trace!(peer_id = ?peer, channel_id = ?channel_id, packet = ?packet, "Enet received");

                    return Ok(EnetOutput::Event(EnetEvent::Receive {
                        peer: peer.id(),
                        channel_id,
                        data: packet.data().to_vec(),
                    }));
                }
                Event::Disconnect { peer, data } => {
                    debug!(peer_id = ?peer, disconnect_data = ?data, "Enet disconnected");

                    return Ok(EnetOutput::Event(EnetEvent::Disconnect {
                        peer: peer.id(),
                        data,
                    }));
                }
            }
        }

        // TODO: dynamically set timeout, see https://github.com/jabuwu/rusty_enet/issues/4
        Ok(EnetOutput::Timeout(
            self.last_now() + Duration::from_millis(50),
        ))
    }

    pub fn handle_input(&mut self, input: EnetInput) -> Result<(), EnetError> {
        match input {
            EnetInput::Timeout(now) => {
                self.set_last_now(now);
            }
            EnetInput::Receive { now, addr, data } => {
                self.set_last_now(now);

                self.enet.socket_mut().write(addr, data.to_vec());
            }
        }

        Ok(())
    }

    fn set_last_now(&self, now: Instant) {
        // This is allowed because we:
        // 1. only we have got access to this mutex
        // 2. don't panic inside this implementation
        #[allow(clippy::unwrap_used)]
        let mut last_now = self.last_now.lock().unwrap();

        *last_now = now;
    }
    fn last_now(&self) -> Instant {
        // This is allowed because we:
        // 1. only we have got access to this mutex
        // 2. don't panic inside this implementation
        #[allow(clippy::unwrap_used)]
        let last_now = self.last_now.lock().unwrap();

        *last_now
    }
}
