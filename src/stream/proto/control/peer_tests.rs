use std::{net::SocketAddr, sync::Arc, time::Duration};

use sans_io_time::Instant;
use tracing::{debug, info};

use crate::{
    ServerVersion,
    crypto::disabled::DisabledCryptoBackend,
    stream::proto::{
        control::{
            packet::{ControlPacket, ControlPacketConfig, EnetChannel, TerminationReason},
            peer::{
                ControlHost, ControlHostConfig, ControlHostEvent, ControlPeerConfig,
                ControlPeerRole,
            },
            stream::{ControlStream, ControlStreamConfig, ControlStreamEvent},
        },
        runtime::{Receive, UdpStream},
    },
};

fn transfer<S, D>(peer_a: (SocketAddr, &mut S), peer_b: (SocketAddr, &mut D), now: Instant)
where
    S: UdpStream,
    D: UdpStream,
{
    let (peer_a_addr, peer_a_stream) = peer_a;
    let (peer_b_addr, peer_b_stream) = peer_b;

    'outer: loop {
        if let Some(transmit) = peer_a_stream.poll_transmit() {
            peer_b_stream
                .handle_receive(
                    now,
                    Receive {
                        source: peer_a_addr,
                        data: &transmit.data,
                    },
                )
                .unwrap();
            continue 'outer;
        }

        if let Some(transmit) = peer_b_stream.poll_transmit() {
            peer_a_stream
                .handle_receive(
                    now,
                    Receive {
                        source: peer_b_addr,
                        data: &transmit.data,
                    },
                )
                .unwrap();
            continue 'outer;
        }

        break;
    }
}

#[test]
fn termination_remains_available_until_delivered() {
    let client_addr: SocketAddr = "127.0.0.1:50000".parse().unwrap();
    let server_addr: SocketAddr = "127.0.0.1:47999".parse().unwrap();
    let version = ServerVersion::new(7, 1, 431, -1);
    let encryption = None;

    let mut now = Instant::ZERO;

    // Create Client
    let mut client = ControlStream::new(
        now,
        ControlStreamConfig {
            server_version: version,
            addr: server_addr,
            sunshine_connect_data: None,
            encryption,
            apollo_permissions: None,
        },
        Arc::new(DisabledCryptoBackend),
    )
    .unwrap();

    // Create Server
    let mut server = ControlHost::new(
        now,
        ControlHostConfig {
            peer_count: 1,
            peer_channel_count: EnetChannel::CHANNEL_COUNT,
        },
        Arc::new(DisabledCryptoBackend),
    )
    .unwrap();

    // Connect client to server
    let mut server_peer = None;
    let mut connected = false;
    for _ in 0..100 {
        transfer((client_addr, &mut client), (server_addr, &mut server), now);

        // Handle Server events
        while let Some(event) = server.poll_event() {
            if let ControlHostEvent::Connected { id, .. } = event {
                server
                    .configure_peer(
                        id,
                        ControlPeerConfig {
                            role: ControlPeerRole::Server,
                            encryption,
                            packets: ControlPacketConfig::new(version, encryption.is_some())
                                .unwrap(),
                        },
                    )
                    .unwrap();
                server_peer = Some(id);
            }
        }

        // Handle Client events
        while let Some(event) = client.poll_event() {
            connected |= matches!(event, ControlStreamEvent::Connect);
        }
        if connected && server_peer.is_some() {
            break;
        }

        // Advance time
        now += Duration::from_millis(10);
        client.handle_timeout(now).unwrap();
        server.handle_timeout(now).unwrap();
    }

    assert!(
        connected,
        "the client and server should be connected after 100 iterations"
    );

    // Send Packet from client to server
    // client
    //     .batch_input(ClientInputEvent::MouseMoveRelative {
    //         delta_x: 1,
    //         delta_y: 2,
    //     })
    //     .unwrap();

    // Send termination packet and disconnect client
    let termination_packet = ControlPacket::ServerTermination {
        reason: TerminationReason::GRACEFUL,
    };
    let (channel, kind) = termination_packet.channel(version);
    server
        .send(
            server_peer.unwrap(),
            channel,
            kind,
            termination_packet.clone(),
        )
        .unwrap();
    server.disconnect(server_peer.unwrap(), 0).unwrap();

    // Poll client for events
    let mut received_termination = false;
    let mut received_disconnect = false;

    for _ in 0..100 {
        if client.can_discard() {
            debug!("can discard client");
            break;
        }

        // Poll client events
        while let Some(event) = client.poll_event() {
            info!(event = ?event, "got event");
            match event {
                ControlStreamEvent::Packet(packet) => {
                    assert_eq!(packet, termination_packet);
                    received_termination = true;
                }
                ControlStreamEvent::Disconnect => {
                    received_disconnect = true;
                }
                _ => {}
            }
        }
        info!(received_disconnect, received_termination, "test state");

        // Transfer data
        transfer((client_addr, &mut client), (server_addr, &mut server), now);

        // Advance time
        now += Duration::from_millis(10);
        client.handle_timeout(now).unwrap();
        server.handle_timeout(now).unwrap();
    }

    // Only after polling client events, the stream can be terminated
    assert!(
        received_termination,
        "termination packet must reach the client"
    );
    assert!(received_disconnect, "disconnect must reach the client");
    assert!(client.can_discard(), "client can be discarded now");
}
