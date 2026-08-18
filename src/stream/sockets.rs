use std::{io, net::UdpSocket};

use socket2::{Domain, Protocol, Socket, Type};
use tracing::{debug, warn};

const RCV_BUFFER_SIZE_MIN: usize = 32767;
const RCV_BUFFER_SIZE_STEP: usize = 16384;

pub fn new_udp_socket(ipv6: bool, recv_buffer_size: Option<usize>) -> io::Result<UdpSocket> {
    let domain = if ipv6 { Domain::IPV6 } else { Domain::IPV4 };

    let socket = Socket::new(domain, Type::DGRAM, Some(Protocol::UDP))?;

    // Try to set the recv buffer size
    // https://github.com/moonlight-stream/moonlight-common-c/blob/e41355ea01670fd4c830b384009d31dd0339a705/src/PlatformSockets.c#L365-L404
    if let Some(mut buffer_size) = recv_buffer_size {
        loop {
            match socket.set_recv_buffer_size(buffer_size) {
                Ok(()) => {
                    debug!(requested = buffer_size, "selected receive buffer size");
                    break;
                }

                Err(err) if buffer_size <= RCV_BUFFER_SIZE_MIN => {
                    warn!(
                        requested = buffer_size,
                        error = %err,
                        "unable to set receive buffer size"
                    );
                    break;
                }

                Err(_)
                    if buffer_size.saturating_sub(RCV_BUFFER_SIZE_STEP) <= RCV_BUFFER_SIZE_MIN =>
                {
                    // Last: try the minimum.
                    buffer_size = RCV_BUFFER_SIZE_MIN;
                }

                Err(_) => {
                    buffer_size -= RCV_BUFFER_SIZE_STEP;
                }
            }
        }
    }

    Ok(socket.into())
}
