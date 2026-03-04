use std::{
    io::{self, ErrorKind as IoError, Read, Write},
    net::{SocketAddr, TcpStream, UdpSocket},
    sync::{
        Arc,
        mpsc::{Receiver, RecvTimeoutError, Sender, channel},
    },
    thread::spawn,
    time::{Duration, Instant},
};

use thiserror::Error;
use tracing::{Level, Span, info_span, instrument};

use crate::stream::{
    MoonlightStreamConfig, MoonlightStreamSettings,
    audio::{AudioConfig, AudioDecoder},
    connection::ConnectionListener,
    proto::{
        MoonlightStreamAction, MoonlightStreamInput, MoonlightStreamOutput, MoonlightStreamProto,
        MoonlightStreamProtoError,
        audio::{AudioStream, AudioStreamInput, AudioStreamOutput},
        control::{
            ControlMessage, ControlStream, ControlStreamEvent, ControlStreamInput,
            ControlStreamOutput,
        },
        video::{VideoStream, VideoStreamInput, VideoStreamOutput},
    },
    std::ringbuffer::RingBuffer,
    video::{ColorSpace, FrameType, VideoDecodeUnit, VideoDecoder, VideoFrameBuffer},
};

// TODO: move decoders into different thread because the network thread NEEDS to be quick to avoid packet losses by the underlying buffer dropping packets

mod ringbuffer;

#[derive(Debug, Error)]
pub enum MoonlightStreamError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("proto: {0}")]
    Proto(#[from] MoonlightStreamProtoError),
}

pub struct MoonlightStream {}

impl MoonlightStream {
    pub fn launch_query_parameters() -> &'static str {
        // TODO: change this to corever=1 when rtsp encryption is supported
        // "&corever=1"
        ""
    }

    pub fn new(
        config: MoonlightStreamConfig,
        settings: MoonlightStreamSettings,
        video_decoder: impl VideoDecoder + Send + 'static,
        audio_decoder: impl AudioDecoder + Send + 'static,
        connection_listener: impl ConnectionListener + Send + 'static,
    ) -> Result<Self, MoonlightStreamError> {
        let span = info_span!("stream");
        let span2 = span.clone();
        let _enter = span2.enter();

        let proto = MoonlightStreamProto::new(Instant::now(), config, settings)?;

        let (sender, receiver) = channel();

        let (_, send_udp) = bind_udp_socket(sender.clone())?;

        spawn(move || {
            let _enter = span.enter();

            proto_thread(
                proto,
                sender,
                receiver,
                send_udp,
                Box::new(audio_decoder),
                Box::new(video_decoder),
                Box::new(connection_listener),
            );
        });

        Ok(Self {})
    }

    pub fn stop(self) {
        // TODO: when dropping the connection should be closed in another thread, only stop should wait until the connection closed successful, maybe with result
    }
}

enum Input {
    TcpConnect(Instant),
    TcpReceive {
        now: Instant,
        data: Vec<u8>,
    },
    TcpDisconnect(Instant),
    UdpReceive {
        now: Instant,
        source: SocketAddr,
        data: Vec<u8>,
    },
    ControlMessage {
        message: ControlMessage,
    },
}

fn bind_udp_socket(
    sender: Sender<Input>,
) -> Result<(Arc<UdpSocket>, Sender<(SocketAddr, Vec<u8>)>), io::Error> {
    let udp = UdpSocket::bind("0.0.0.0:0")?;
    let udp = Arc::new(udp);

    let (send_udp, receive_udp) = channel();
    spawn({
        let udp = udp.clone();
        move || {
            udp_send_thread(udp, receive_udp);
        }
    });

    spawn({
        let udp = udp.clone();
        move || {
            udp_receive_thread(udp, sender);
        }
    });

    Ok((udp, send_udp))
}
fn udp_send_thread(socket: Arc<UdpSocket>, receiver: Receiver<(SocketAddr, Vec<u8>)>) {
    while let Ok((addr, data)) = receiver.recv() {
        socket.send_to(&data, addr).unwrap();
    }
}
fn udp_receive_thread(socket: Arc<UdpSocket>, sender: Sender<Input>) {
    let mut buffer = vec![0u8; 4096];

    loop {
        let (len, addr) = socket.recv_from(&mut buffer).unwrap();

        let slice = &buffer[0..len];
        if sender
            .send(Input::UdpReceive {
                now: Instant::now(),
                source: addr,
                data: slice.to_vec(),
            })
            .is_err()
        {
            return;
        }
    }
}

fn udp_queue_receive_thread(
    socket: Arc<UdpSocket>,
    queue: Arc<RingBuffer>,
    max_packet_size: usize,
) {
    let mut buffer = vec![0; max_packet_size];
    loop {
        let len = socket.recv(&mut buffer).unwrap();

        queue.push(&buffer[0..len]);
    }
}

fn tcp_thread(addr: SocketAddr, send_tcp: Sender<Input>, receiver: Receiver<Vec<u8>>) {
    let stream = TcpStream::connect(addr).unwrap();
    stream.set_nodelay(true).unwrap();

    match send_tcp.send(Input::TcpConnect(Instant::now())) {
        Ok(_) => {}
        Err(_) => todo!(),
    }

    let stream = Arc::new(stream);

    spawn({
        let stream = stream.clone();

        move || {
            tcp_receive_thread(stream, send_tcp);
        }
    });
    let mut stream = &stream as &TcpStream;
    let stream = &mut stream as &mut &TcpStream;

    loop {
        while let Ok(data) = receiver.recv() {
            stream.write_all(&data).unwrap();
        }
    }
}
fn tcp_receive_thread(stream: Arc<TcpStream>, mut sender: Sender<Input>) {
    let mut stream = &stream as &TcpStream;
    let stream = &mut stream as &mut &TcpStream;

    let mut buffer = vec![0u8; 4096];

    loop {
        let len = match stream.read(&mut buffer) {
            Ok(len) => len,
            Err(err) if err.kind() == IoError::ConnectionReset => 0,
            Err(err) => todo!("{}", err),
        };

        if len == 0 {
            let _ = sender.send(Input::TcpDisconnect(Instant::now()));
            return;
        }

        let slice = &buffer[0..len];

        match sender.send(Input::TcpReceive {
            now: Instant::now(),
            data: slice.to_vec(),
        }) {
            Ok(_) => {}
            Err(_) => todo!(),
        }
    }
}

#[instrument(level = Level::DEBUG, skip_all)]
fn proto_thread(
    mut proto: MoonlightStreamProto,
    sender: Sender<Input>,
    receiver: Receiver<Input>,
    send_udp: Sender<(SocketAddr, Vec<u8>)>,
    // TODO: should those be impls?
    audio_decoder: Box<dyn AudioDecoder + Send + 'static>,
    video_decoder: Box<dyn VideoDecoder + Send + 'static>,
    connection_listener: Box<dyn ConnectionListener + Send + 'static>,
) {
    let mut send_tcp = None;
    let (send_control_message, receive_control_message) = channel();

    let mut audio_decoder = Some(audio_decoder);
    let mut video_decoder = Some(video_decoder);
    let mut connection_listener = Some((connection_listener, receive_control_message));

    loop {
        let timeout = match proto.poll_output().unwrap() {
            MoonlightStreamOutput::Action(action) => match action {
                MoonlightStreamAction::ConnectTcp { addr } => {
                    let sender = sender.clone();
                    let (tcp_sender, tcp_receiver) = channel();

                    spawn(move || {
                        tcp_thread(addr, sender, tcp_receiver);
                    });

                    send_tcp = Some(tcp_sender);
                    continue;
                }
                MoonlightStreamAction::SendTcp { data } => {
                    send_tcp.as_mut().unwrap().send(data).unwrap();
                    continue;
                }
                MoonlightStreamAction::SendUdp { to, data } => {
                    send_udp.send((to, data)).unwrap();
                    continue;
                }
                MoonlightStreamAction::StartAudioStream { addr, audio_stream } => {
                    let audio_decoder = audio_decoder.take().unwrap();

                    let udp = Arc::new(UdpSocket::bind("0.0.0.0:0").unwrap());

                    udp.connect(addr).unwrap();

                    let (send_udp, send_udp_receiver) = channel();
                    spawn({
                        let udp = udp.clone();
                        move || {
                            udp_send_thread(udp, send_udp_receiver);
                        }
                    });

                    let span = Span::current();
                    spawn(move || {
                        let _enter = span.enter();

                        audio_thread(audio_stream, udp, send_udp, audio_decoder);
                    });
                    continue;
                }
                MoonlightStreamAction::StartVideoStream { addr, video_stream } => {
                    let video_decoder = video_decoder.take().unwrap();

                    let udp = Arc::new(UdpSocket::bind("0.0.0.0:0").unwrap());

                    udp.connect(addr).unwrap();

                    let (send_udp, send_udp_receiver) = channel();
                    spawn({
                        let udp = udp.clone();
                        move || {
                            udp_send_thread(udp, send_udp_receiver);
                        }
                    });

                    let span = Span::current();
                    spawn(move || {
                        let _enter = span.enter();

                        video_thread(video_stream, udp, send_udp, video_decoder);
                    });
                    continue;
                }
                MoonlightStreamAction::StartControlStream {
                    addr,
                    control_stream,
                } => {
                    let (connection_listener, receiver) = connection_listener.take().unwrap();

                    let udp = Arc::new(UdpSocket::bind("0.0.0.0:0").unwrap());

                    udp.connect(addr).unwrap();

                    // TODO: maybe the udp sender should have the control stream?
                    spawn({
                        let udp = udp.clone();
                        let send_control_message = send_control_message.clone();
                        move || {
                            udp_receive_thread(udp, send_control_message);
                        }
                    });

                    let span = Span::current();
                    spawn(move || {
                        let _enter = span.enter();

                        control_thread(control_stream, udp, receiver, connection_listener);
                    });
                    continue;
                }
                MoonlightStreamAction::SendControlMessage { message } => {
                    send_control_message
                        .send(Input::ControlMessage { message })
                        .unwrap();
                    continue;
                }
            },
            MoonlightStreamOutput::Event(event) => {
                println!("{event:?}");

                continue;
            }
            MoonlightStreamOutput::Timeout(timeout) => timeout,
        };

        let Some(duration) = timeout.checked_duration_since(Instant::now()) else {
            proto
                .handle_input(MoonlightStreamInput::Timeout(Instant::now()))
                .unwrap();
            continue;
        };

        let input = match receiver.recv_timeout(duration) {
            Ok(input) => input,
            Err(RecvTimeoutError::Timeout) => {
                proto
                    .handle_input(MoonlightStreamInput::Timeout(Instant::now()))
                    .unwrap();
                continue;
            }
            // TODO
            Err(RecvTimeoutError::Disconnected) => todo!(),
        };

        match input {
            Input::TcpConnect(time) => {
                proto
                    .handle_input(MoonlightStreamInput::TcpConnect(time))
                    .unwrap();
            }
            Input::TcpReceive { now, data } => {
                proto
                    .handle_input(MoonlightStreamInput::TcpReceive { now, data: &data })
                    .unwrap();
            }
            Input::TcpDisconnect(time) => {
                proto
                    .handle_input(MoonlightStreamInput::TcpDisconnect(time))
                    .unwrap();
            }
            Input::UdpReceive { now, source, data } => {
                proto
                    .handle_input(MoonlightStreamInput::UdpReceive {
                        now,
                        source,
                        data: &data,
                    })
                    .unwrap();
            }
            Input::ControlMessage { .. } => unreachable!(),
        }
    }
}

// TODO: audio queue using synchronized buffer

#[instrument(level = Level::DEBUG, skip_all)]
fn audio_thread(
    mut audio_stream: AudioStream,
    socket: Arc<UdpSocket>,
    send_udp: Sender<(SocketAddr, Vec<u8>)>,
    mut audio_decoder: Box<dyn AudioDecoder + Send + 'static>,
) {
    let mut started = false;
    let mut buffer = vec![0u8; 4096];

    loop {
        let timeout = match audio_stream.poll_output().unwrap() {
            AudioStreamOutput::Send { to, data } => {
                send_udp.send((to, data)).unwrap();
                continue;
            }
            AudioStreamOutput::Setup { opus_config } => {
                // TODO: audio config?
                audio_decoder.setup(AudioConfig::STEREO, opus_config);

                continue;
            }
            AudioStreamOutput::AudioSample(sample) => {
                if !started {
                    audio_decoder.start();

                    started = true;
                }

                audio_decoder.decode_and_play_sample(sample);

                // TODO: call stop somewhere when shutting down
                continue;
            }
            AudioStreamOutput::Timeout(timeout) => timeout,
        };

        let Some(duration) = timeout.checked_duration_since(Instant::now()) else {
            audio_stream
                .handle_input(AudioStreamInput::Timeout(Instant::now()))
                .unwrap();
            continue;
        };

        socket.set_read_timeout(Some(duration)).unwrap();

        match socket.recv_from(&mut buffer) {
            Ok((len, addr)) => {
                let slice = &buffer[0..len];

                audio_stream
                    .handle_input(AudioStreamInput::Receive {
                        now: Instant::now(),
                        from: addr,
                        data: slice,
                    })
                    .unwrap();
            }
            Err(err) if err.kind() == IoError::WouldBlock || err.kind() == IoError::TimedOut => {
                audio_stream
                    .handle_input(AudioStreamInput::Timeout(Instant::now()))
                    .unwrap();
            }
            Err(err) => {
                todo!("{}", err)
            }
        };
    }
}

// TODO: where to put this?
// https://github.com/moonlight-stream/moonlight-common-c/blob/2a5a1f3e8a57cbbb316ed7dfff3a3965c2e77d25/src/RtpVideoQueue.c#L16-L17
const VIDEO_SECOND_TO_TIMESTAMP: f64 = 90000.0;

#[instrument(level = Level::DEBUG, skip_all)]
fn video_thread(
    mut video_stream: VideoStream,
    socket: Arc<UdpSocket>,
    send_udp: Sender<(SocketAddr, Vec<u8>)>,
    mut video_decoder: Box<dyn VideoDecoder + Send + 'static>,
    // TODO: get the video information
) {
    let mut started = false;
    // TODO: max packet size
    let max_packet_size = 4096;
    let mut buffer = vec![0; max_packet_size];
    let queue = Arc::new(RingBuffer::new(100, max_packet_size));

    udp_queue_receive_thread(socket, queue.clone(), max_packet_size);

    loop {
        let timeout = match video_stream.poll_output().unwrap() {
            VideoStreamOutput::SendUdp { to, data } => {
                send_udp.send((to, data)).unwrap();
                continue;
            }
            VideoStreamOutput::VideoFrame(frame) => {
                if !started {
                    // TODO: setup, maybe when setting up sdp?
                    // video_decoder.setup(setup);

                    video_decoder.start();
                    started = true;
                }

                let mut buffers = Vec::new();
                for buffer in &frame.buffers {
                    buffers.push(VideoFrameBuffer {
                        buffer_type: buffer.buffer_type,
                        data: buffer.data.as_slice(),
                    });
                }

                // TODO

                video_decoder.submit_decode_unit(VideoDecodeUnit {
                    frame_number: frame.frame_number as i32,
                    // TODO: frame type
                    frame_type: FrameType::PFrame,
                    frame_processing_latency: None,
                    // TODO: timestamp
                    timestamp: Duration::ZERO,
                    hdr_active: false,
                    color_space: ColorSpace::Rec709,
                    buffers: buffers.as_slice(),
                });

                // TODO: stop video decoder
                continue;
            }
            VideoStreamOutput::Timeout(timeout) => timeout,
            // TODO
            _ => todo!(),
        };

        let Some(duration) = timeout.checked_duration_since(Instant::now()) else {
            video_stream
                .handle_input(VideoStreamInput::Timeout(Instant::now()))
                .unwrap();
            continue;
        };

        if let Some(len) = queue.pop(&mut buffer, Some(duration)) {
            let slice = &buffer[0..len];

            video_stream
                .handle_input(VideoStreamInput::Receive {
                    now: Instant::now(),
                    data: slice,
                })
                .unwrap();
        }
    }
}

#[instrument(level = Level::DEBUG, skip_all)]
fn control_thread(
    mut control_stream: ControlStream,
    socket: Arc<UdpSocket>,
    receiver: Receiver<Input>,
    mut connection_listener: Box<dyn ConnectionListener + Send + 'static>,
) {
    loop {
        let timeout = match control_stream.poll_output().unwrap() {
            ControlStreamOutput::Event(event) => match event {
                ControlStreamEvent::OnPacket(packet) => {
                    todo!()
                }
            },
            ControlStreamOutput::Send { to, data } => {
                socket.send_to(&data, to).unwrap();
                continue;
            }
            ControlStreamOutput::Timeout(timeout) => timeout,
        };

        let Some(duration) = timeout.checked_duration_since(Instant::now()) else {
            control_stream
                .handle_input(ControlStreamInput::Timeout(Instant::now()))
                .unwrap();
            continue;
        };

        match receiver.recv_timeout(duration) {
            Ok(input) => match input {
                Input::ControlMessage { message } => {
                    control_stream
                        .handle_input(ControlStreamInput::Message(message))
                        .unwrap();
                }
                Input::UdpReceive { now, source, data } => {
                    control_stream
                        .handle_input(ControlStreamInput::Receive {
                            now,
                            addr: source,
                            data: &data,
                        })
                        .unwrap();
                }
                Input::TcpReceive { .. } => unreachable!(),
                Input::TcpDisconnect(_) => unreachable!(),
                Input::TcpConnect(_) => unreachable!(),
            },
            Err(RecvTimeoutError::Timeout) => {
                control_stream
                    .handle_input(ControlStreamInput::Timeout(Instant::now()))
                    .unwrap();
            }
            // TODO
            Err(RecvTimeoutError::Disconnected) => todo!(),
        }
    }
}
