use fec_rs::ReedSolomon;

use crate::stream::proto::audio::packet::{RTP_AUDIO_DATA_SHARDS, RTP_AUDIO_FEC_SHARDS};

mod packet;

pub mod depayloader;
pub mod payloader;
pub mod stream;

fn create_audio_reed_solomon() -> ReedSolomon {
    // Normal rs implementation don't generate a correct rs matrix: https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/RtpAudioQueue.c#L52-L59
    let parity: [u8; 8] = [0x77, 0x40, 0x38, 0x0e, 0xc7, 0xa7, 0x0d, 0x6c];

    // This won't panic because all values are controlled by us and are correct for the rs implementation
    #[allow(clippy::unwrap_used)]
    let mut reed_solomon = ReedSolomon::new(RTP_AUDIO_DATA_SHARDS, RTP_AUDIO_FEC_SHARDS).unwrap();

    // This won't panic because all values are controlled by us and are correct for the rs implementation
    #[allow(clippy::unwrap_used)]
    reed_solomon.set_parity_matrix(&parity).unwrap();

    reed_solomon
}
