use std::array;

use reed_solomon_erasure::galois_8::ReedSolomon;
use thiserror::Error;

use crate::stream::proto::audio::{
    create_audio_reed_solomon,
    packet::{
        AudioFecHeader, RTP_AUDIO_DATA_SHARDS, RTP_AUDIO_HEADER, RTP_PAYLOAD_TYPE_AUDIO,
        RTP_PAYLOAD_TYPE_AUDIO_FEC, RtpAudioHeader,
    },
};

pub struct AudioPayloaderConfig {
    pub fec: bool,
    /// The size of one opus frame in bytes
    pub frame_len: usize,
    // TODO: encryption
}

#[derive(Debug, Error, PartialEq)]
pub enum AudioPayloaderError {
    /// This frame is bigger than allowed
    #[error("opus frame has invalid size")]
    InvalidFrameSize,
}

#[derive(Debug, PartialEq)]
enum State {
    PushFrame,
    PollPacket,
    PollFec1,
    PollFec2,
}

pub struct AudioPayloader {
    reed_solomon: Option<ReedSolomon>,
    frame_len: usize,
    base_timestamp: u32,
    sequence_number: u16,
    data_shards: [Vec<u8>; 4],
    fec_packets: [Vec<u8>; 2],
    next_packet: Vec<u8>,
    state: State,
}

impl AudioPayloader {
    pub fn new(config: AudioPayloaderConfig) -> Self {
        debug_assert!(config.frame_len > 0);

        Self {
            reed_solomon: config.fec.then(create_audio_reed_solomon),
            frame_len: config.frame_len,
            data_shards: array::from_fn(|_| vec![0; config.frame_len]),
            sequence_number: 0,
            base_timestamp: 0,
            next_packet: vec![0; RtpAudioHeader::SIZE + config.frame_len],
            fec_packets: array::from_fn(|_| {
                vec![0; RtpAudioHeader::SIZE + AudioFecHeader::SIZE + config.frame_len]
            }),
            state: State::PushFrame,
        }
    }

    /// Pushes one opus frame to the payloader.
    ///
    /// After each call to AudioPayloader::push_frame AudioPayloader::poll_packet must be called until it returns None or an error.
    pub fn push_frame(&mut self, timestamp: u32, frame: &[u8]) -> Result<(), AudioPayloaderError> {
        // TODO: maybe lift this limitation in the future?
        debug_assert_eq!(
            self.state,
            State::PushFrame,
            "You must call AudioPayloader::poll_packet until it returns None after a call to AudioPayloader::push_frame!"
        );
        if frame.len() != self.frame_len {
            return Err(AudioPayloaderError::InvalidFrameSize);
        }

        let rtp_header = RtpAudioHeader {
            header: RTP_AUDIO_HEADER,
            packet_type: RTP_PAYLOAD_TYPE_AUDIO,
            sequence_number: self.sequence_number,
            timestamp,
            ssrc: 0,
        };

        #[allow(clippy::unwrap_used)]
        rtp_header.serialize(
            self.next_packet[0..RtpAudioHeader::SIZE]
                .as_mut_array()
                .unwrap(),
        );

        self.next_packet[RtpAudioHeader::SIZE..(RtpAudioHeader::SIZE + self.frame_len)]
            .copy_from_slice(frame);

        // Insert into data shards for fec construction
        let shard_index = (self.sequence_number % 4) as usize;
        self.data_shards[shard_index].copy_from_slice(frame);

        if shard_index == 0 {
            self.base_timestamp = timestamp;
        }

        self.sequence_number = self.sequence_number.wrapping_add(1);

        self.state = State::PollPacket;

        Ok(())
    }

    pub fn poll_packet(&mut self) -> Result<Option<&[u8]>, AudioPayloaderError> {
        match self.state {
            State::PushFrame => Ok(None),
            State::PollPacket => {
                self.state = State::PollFec1;

                Ok(Some(
                    &self.next_packet[0..(RtpAudioHeader::SIZE + self.frame_len)],
                ))
            }
            State::PollFec1 => {
                // Check if we need fec construction?

                // IMPORTANT: https://github.com/moonlight-stream/moonlight-common-c/blob/2a5a1f3e8a57cbbb316ed7dfff3a3965c2e77d25/src/RtpAudioQueue.c#L269-L271
                // The FEC blocks must start on a RTP_DATA_SHARDS boundary for our queuing logic to work. This isn't
                // the case for older versions of GeForce Experience (at least 3.13). Disable the FEC logic if this
                // invariant is validated.
                if let Some(reed_solomon) = &self.reed_solomon
                    && self
                        .sequence_number
                        .wrapping_sub(4)
                        .is_multiple_of(RTP_AUDIO_DATA_SHARDS as u16)
                {
                    self.state = State::PollFec2;

                    let base_sequence_number = self.sequence_number.wrapping_sub(4);
                    debug_assert!(
                        base_sequence_number.is_multiple_of(RTP_AUDIO_DATA_SHARDS as u16),
                        "Fec Block must start on a RTP_AUDIO_DATA_SHARDS boundary! Current Sequence Number: {}, Calculated Base Sequence Number: {base_sequence_number}",
                        self.sequence_number
                    );

                    // -- Construct parity shards directly into the buffer

                    let parity_payload_range = (RtpAudioHeader::SIZE + AudioFecHeader::SIZE)
                        ..(RtpAudioHeader::SIZE + AudioFecHeader::SIZE + self.frame_len);

                    // We need an iter to get around the borrow checker
                    let mut fec_packets = self.fec_packets.iter_mut();

                    // fec_packets always has a length 2 to -> no panic
                    #[allow(clippy::unwrap_used)]
                    let mut parity_shards = [
                        &mut fec_packets.next().unwrap()[parity_payload_range.clone()],
                        &mut fec_packets.next().unwrap()[parity_payload_range],
                    ];

                    // We know that every frame has the same size -> this cannot panic
                    #[allow(clippy::unwrap_used)]
                    reed_solomon
                        .encode_sep(&self.data_shards, &mut parity_shards)
                        .unwrap();

                    // -- Construct individual rtp headers
                    let mut rtp_header = RtpAudioHeader {
                        header: RTP_AUDIO_HEADER,
                        packet_type: RTP_PAYLOAD_TYPE_AUDIO_FEC,
                        // We are always incrementing after having constructed the packet -> we don't need increment here
                        sequence_number: self.sequence_number,
                        timestamp: 0,
                        ssrc: 0,
                    };

                    #[allow(clippy::unwrap_used)]
                    rtp_header.serialize(
                        self.fec_packets[0][0..RtpAudioHeader::SIZE]
                            .as_mut_array()
                            .unwrap(),
                    );

                    rtp_header = RtpAudioHeader {
                        header: RTP_AUDIO_HEADER,
                        packet_type: RTP_PAYLOAD_TYPE_AUDIO_FEC,
                        sequence_number: self.sequence_number.wrapping_add(1),
                        timestamp: 0,
                        ssrc: 0,
                    };

                    #[allow(clippy::unwrap_used)]
                    rtp_header.serialize(
                        self.fec_packets[1][0..RtpAudioHeader::SIZE]
                            .as_mut_array()
                            .unwrap(),
                    );

                    // -- Construct fec packet headers
                    let mut fec_header = AudioFecHeader {
                        fec_shard_index: 0,
                        payload_type: RTP_PAYLOAD_TYPE_AUDIO,
                        base_sequence_number,
                        base_timestamp: self.base_timestamp,
                        ssrc: 0,
                    };

                    #[allow(clippy::unwrap_used)]
                    fec_header.serialize(
                        self.fec_packets[0]
                            [RtpAudioHeader::SIZE..(RtpAudioHeader::SIZE + AudioFecHeader::SIZE)]
                            .as_mut_array()
                            .unwrap(),
                    );

                    fec_header.fec_shard_index = 1;

                    #[allow(clippy::unwrap_used)]
                    fec_header.serialize(
                        self.fec_packets[1]
                            [RtpAudioHeader::SIZE..(RtpAudioHeader::SIZE + AudioFecHeader::SIZE)]
                            .as_mut_array()
                            .unwrap(),
                    );

                    Ok(Some(&self.fec_packets[0]))
                } else {
                    self.state = State::PushFrame;

                    Ok(None)
                }
            }
            State::PollFec2 => {
                self.state = State::PushFrame;

                Ok(Some(&self.fec_packets[1]))
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn set_sequence_number(&mut self, sequence_number: u16) {
        self.sequence_number = sequence_number;
    }
}
