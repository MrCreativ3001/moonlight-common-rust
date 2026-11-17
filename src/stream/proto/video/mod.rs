use crate::stream::proto::video::packet::{EncryptedVideoHeader, RtpVideoHeader};

pub mod frame;
#[allow(unused)]
mod nal;

mod packet;

pub mod depayloader;
pub mod payloader;
pub mod stream;

pub(crate) fn sdp_packet_size_into_actual_packet_size(
    encrypted: bool,
    packet_size: usize,
) -> usize {
    packet_size
        + if encrypted {
            EncryptedVideoHeader::SIZE + RtpVideoHeader::SIZE
        } else {
            RtpVideoHeader::SIZE
        }
}
