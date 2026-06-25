use std::{net::SocketAddr, pin::pin, time::Duration};

use futures::{FutureExt, future::Fuse, select, try_join};
use sans_io_time::Instant;
use thiserror::Error;

use crate::stream::{
    HostFeatures, MoonlightStreamConfig, MoonlightStreamSettings,
    audio::OpusMultistreamConfig,
    proto::{
        DynCryptoBackend, MoonlightStreamInput, MoonlightStreamProtoError, MoonlightStreamSetup,
        MoonlightStreamSetupOutput,
        audio::{AudioStream, AudioStreamError},
        control::{ControlStream, ControlStreamEvent, peer::ControlError},
        microphone::foundation::{FoundationMicStream, FoundationMicStreamError},
        video::{VideoStream, VideoStreamError},
    },
    video::{VideoCapabilities, VideoSetup},
};

pub trait UdpStream: Send + Sync {
    type Error;

    type Event;

    fn pending_send(&self) -> Option<(SocketAddr, &[u8])>;
    fn consume_send(&mut self);

    fn poll_timeout(&self) -> Option<Instant>;

    fn poll_event(&mut self) -> Option<Self::Event>;

    fn handle_receive(
        &mut self,
        now: Instant,
        addr: SocketAddr,
        data: &[u8],
    ) -> Result<(), Self::Error>;

    fn handle_timeout(&mut self, now: Instant) -> Result<(), Self::Error>;
}

pub trait Runtime: Clone {
    type Error;

    fn now(&self) -> Instant;

    fn sleep_until(&self, deadline: Instant) -> impl Future<Output = ()>;

    type TcpStream: AsyncTcpStream<Self::Error>;

    fn connect_tcp_stream(
        &self,
        addr: SocketAddr,
    ) -> impl Future<Output = Result<Self::TcpStream, Self::Error>>;

    type UdpSocket: AsyncUdpSocket<Self::Error>;

    fn bind_udp_socket(&self) -> impl Future<Output = Result<Self::UdpSocket, Self::Error>>;
}
pub trait AsyncTcpStream<Error> {
    /// # Cancel Safety
    /// This function is cancel safe.
    fn write(&mut self, buffer: &[u8]) -> impl Future<Output = Result<usize, Error>>;

    /// # Cancel Safety
    /// This function is not cancel safe.
    /// If it is cancelled the buffer might be partially written.
    fn write_all(&mut self, buffer: &[u8]) -> impl Future<Output = Result<(), Error>> {
        async {
            let mut offset = 0;
            while offset < buffer.len() {
                offset += self.write(buffer).await?;
            }
            Ok(())
        }
    }

    /// # Cancel Safety
    /// This function is cancel safe.
    fn read(&mut self, buffer: &mut [u8]) -> impl Future<Output = Result<usize, Error>>;
}

#[derive(Debug, Error)]
pub enum MoonlightStreamError<IoError> {
    #[error("io: {0}")]
    Io(IoError),
    #[error("setup: {0}")]
    Setup(#[from] MoonlightStreamProtoError),
    #[error("audio: {0}")]
    Audio(#[from] AudioStreamError),
    #[error("video: {0}")]
    Video(#[from] VideoStreamError),
    #[error("control: {0}")]
    Control(#[from] ControlError),
    #[error("foundation mic: {0}")]
    FoundationMic(#[from] FoundationMicStreamError),
    #[error("connection timed out")]
    ConnectionTimeout,
}

pub struct ConnectedStream<Rt>
where
    Rt: Runtime,
{
    pub host_features: HostFeatures,
    pub audio_setup: OpusMultistreamConfig,
    pub audio_stream: AsyncUdpDriver<Rt, AudioStream>,
    pub video_setup: VideoSetup,
    pub video_stream: AsyncUdpDriver<Rt, VideoStream>,
    pub control_stream: AsyncUdpDriver<Rt, ControlStream>,
    pub foundation_mic_stream: Option<AsyncUdpDriver<Rt, FoundationMicStream>>,
}

pub async fn connect_stream<Rt>(
    runtime: &Rt,
    config: MoonlightStreamConfig,
    settings: MoonlightStreamSettings,
    crypto_backend: DynCryptoBackend,
    video_capabilities: VideoCapabilities,
) -> Result<ConnectedStream<Rt>, MoonlightStreamError<Rt::Error>>
where
    Rt: Runtime,
    MoonlightStreamError<Rt::Error>: From<Rt::Error>,
{
    let mut setup = MoonlightStreamSetup::new(
        runtime.now(),
        config,
        settings,
        crypto_backend,
        video_capabilities,
    )?;

    let mut buffer = vec![0; 4096];
    let mut tcp_stream = None;

    let mut host_features = HostFeatures::default();

    let mut audio_setup = None;
    let mut video_setup = None;

    let mut audio_stream = None;
    let mut video_stream = None;
    let mut control_stream = None;
    let mut foundation_mic_stream = None;

    loop {
        let timeout = match setup.poll_output()? {
            MoonlightStreamSetupOutput::TcpConnect { addr } => {
                tcp_stream = Some(
                    runtime
                        .connect_tcp_stream(addr)
                        .await
                        .map_err(MoonlightStreamError::Io)?,
                );
                continue;
            }
            MoonlightStreamSetupOutput::TcpWrite { data } => {
                let tcp_stream = tcp_stream.as_mut().expect("tcp stream");

                tcp_stream
                    .write_all(&data)
                    .await
                    .map_err(MoonlightStreamError::Io)?;
                continue;
            }
            MoonlightStreamSetupOutput::Timeout(timeout) => timeout,
            MoonlightStreamSetupOutput::StartAudioStream {
                config,
                audio_stream: new_audio_stream,
            } => {
                audio_setup = Some(config);
                audio_stream = Some(new_audio_stream);

                continue;
            }
            MoonlightStreamSetupOutput::StartVideoStream {
                setup,
                video_stream: new_video_stream,
            } => {
                video_setup = Some(setup);
                video_stream = Some(new_video_stream);

                continue;
            }
            MoonlightStreamSetupOutput::StartControlStream {
                control_stream: new_control_stream,
            } => {
                control_stream = Some(new_control_stream);

                continue;
            }
            MoonlightStreamSetupOutput::FoundationStartMic {
                mic_stream: new_foundation_mic_stream,
            } => {
                foundation_mic_stream = Some(new_foundation_mic_stream);

                continue;
            }
            MoonlightStreamSetupOutput::Connected { features } => {
                host_features = features;
                break;
            }
        };

        let len = {
            let read = tcp_stream
                .as_mut()
                .map(|x| x.read(&mut buffer).fuse())
                .unwrap_or(Fuse::terminated());
            let mut read = pin!(read);

            select! {
                _ = runtime.sleep_until(timeout).fuse() => {
                    setup.handle_input(MoonlightStreamInput::Timeout(runtime.now()))?;
                    continue;
                }
                result = read => result.map_err(MoonlightStreamError::Io)?
            }
        };

        let now = runtime.now();
        if len == 0 {
            setup.handle_input(MoonlightStreamInput::TcpDisconnected(now))?;
        } else {
            setup.handle_input(MoonlightStreamInput::TcpReceive {
                now,
                data: &buffer[0..len],
            })?;
        }
    }

    let (audio_stream, video_stream, mut control_stream, foundation_mic_stream) = try_join!(
        AsyncUdpDriver::bind(runtime.clone(), audio_stream.expect("audio stream")),
        AsyncUdpDriver::bind(runtime.clone(), video_stream.expect("video stream")),
        AsyncUdpDriver::bind(runtime.clone(), control_stream.expect("control stream")),
        async {
            if let Some(foundation_mic_stream) = foundation_mic_stream {
                AsyncUdpDriver::bind(runtime.clone(), foundation_mic_stream)
                    .await
                    .map(Some)
            } else {
                Ok(None)
            }
        }
    )
    .map_err(MoonlightStreamError::Io)?;

    // Wait for enet connection
    match control_stream
        .next_event::<MoonlightStreamError<Rt::Error>>()
        .await?
    {
        ControlStreamEvent::Connect => {
            // fallthrough
        }
        _ => unreachable!(),
    }

    Ok(ConnectedStream {
        host_features,
        audio_setup: audio_setup.expect("audio setup"),
        audio_stream,
        video_setup: video_setup.expect("video setup"),
        video_stream,
        control_stream,
        foundation_mic_stream,
    })
}

pub trait AsyncUdpSocket<Error>: Sized {
    /// # Cancel Safety
    /// This function is cancel safe.
    /// If it is cancelled no packet was consumed.
    fn recv(&self, buffer: &mut [u8]) -> impl Future<Output = Result<(SocketAddr, usize), Error>>;

    /// # Cancel Safety
    /// This function is cancel safe.
    fn writable(&self) -> impl Future<Output = Result<(), Error>>;

    fn try_send(&self, addr: SocketAddr, buffer: &[u8]) -> Result<bool, Error>;
}

pub struct AsyncUdpDriver<Rt, Stream>
where
    Rt: Runtime,
{
    runtime: Rt,
    socket: Rt::UdpSocket,
    stream: Stream,
    read_buffer: Vec<u8>,
}

impl<Rt, Stream> AsyncUdpDriver<Rt, Stream>
where
    Rt: Runtime,
    Stream: UdpStream,
{
    pub fn new(runtime: Rt, socket: Rt::UdpSocket, stream: Stream) -> Self {
        Self {
            runtime,
            socket,
            stream,
            read_buffer: vec![0; 4096],
        }
    }

    pub async fn bind(runtime: Rt, stream: Stream) -> Result<Self, Rt::Error> {
        let socket = runtime.bind_udp_socket().await?;

        Ok(Self::new(runtime, socket, stream))
    }

    pub fn into_raw(self) -> (Rt::UdpSocket, Stream) {
        (self.socket, self.stream)
    }

    pub fn stream_mut(&mut self) -> &mut Stream {
        &mut self.stream
    }
    pub fn stream(&self) -> &Stream {
        &self.stream
    }

    /// # Cancel Safety
    /// This function is cancel safe.
    /// This means that no event has been consumed and no state is lost when this function is cancelled.
    pub async fn next_event<Error>(&mut self) -> Result<Stream::Event, Error>
    where
        Error: From<Rt::Error> + From<Stream::Error>,
    {
        loop {
            if let Some(event) = self.stream.poll_event() {
                return Ok(event);
            }

            let timeout = self
                .stream
                .poll_timeout()
                .unwrap_or(self.runtime.now() + Duration::from_secs(1));
            let timeout = self.runtime.sleep_until(timeout);

            let writable = self
                .stream
                .pending_send()
                .map(|_| self.socket.writable().fuse())
                .unwrap_or(Fuse::terminated());
            let writable = pin!(writable);

            select! {
                result = writable.fuse() => {
                    result?;

                    while let Some((addr,pending_send)) = self.stream.pending_send() {
                        if !self.socket.try_send(addr, pending_send)? {
                            break;
                        }
                        self.stream.consume_send();
                    }
                },
                result = self.socket.recv(&mut self.read_buffer).fuse() => {
                    let (addr, len) = result?;

                    self.stream.handle_receive(self.runtime.now(), addr, &self.read_buffer[0..len])?;
                }
                _ = timeout.fuse() => {
                    self.stream.handle_timeout(self.runtime.now())?;
                }
            }
        }
    }
}
