use std::collections::{BTreeMap, VecDeque};

use reed_solomon_erasure::galois_8::ReedSolomon;
use thiserror::Error;

use crate::stream::proto::audio::{
    create_audio_reed_solomon,
    packet::{
        AudioFecHeader, INVALID_OPUS_HEADER, RTP_AUDIO_DATA_SHARDS, RTP_AUDIO_HEADER,
        RTP_AUDIO_TOTAL_SHARDS, RTP_PAYLOAD_TYPE_AUDIO, RTP_PAYLOAD_TYPE_AUDIO_FEC, RtpAudioHeader,
    },
};

use tracing::{Level, instrument, warn};

#[derive(Debug, Error, PartialEq)]
pub enum AudioDepayloaderError {
    #[error("buffer too small")]
    BufferTooSmall,
    #[error("reed solomon: {0}")]
    ReedSolomon(#[from] reed_solomon_erasure::Error),
}

// TODO: this should also handle decryption
// TODO: make a cap for the amount of fec packets and the amount of packets that can be buffered
// TODO: maybe warn if this happens? https://github.com/moonlight-stream/moonlight-common-c/blob/master/src/RtpAudioQueue.c#L269-L271

#[derive(Debug)]
pub struct AudioDepayloaderConfig {
    /// See: https://github.com/moonlight-stream/moonlight-common-c/blob/3a377e7d7be7776d68a57828ae22283144285f90/src/RtpAudioQueue.c#L28-L44
    pub fec: bool,
}

#[derive(Debug, Clone)]
struct DataPacket {
    timestamp: u32,
    payload: Vec<u8>,
}

impl DataPacket {
    fn to_sample(&self) -> AudioSample {
        AudioSample {
            timestamp: self.timestamp,
            buffer: self.payload.clone(),
        }
    }
}

#[derive(Debug, Clone)]
struct FecPacket {
    header: AudioFecHeader,
    payload: Vec<u8>,
}

#[derive(Debug)]
pub struct AudioDepayloader {
    current_sequence_number: u16,
    // TODO: don't deallocate those Vec's but reuse them
    data_packets: BTreeMap<u16, DataPacket>,
    fec_packets: VecDeque<FecPacket>,
    fec_decoder: Option<ReedSolomon>,
}

impl AudioDepayloader {
    #[instrument(level = Level::DEBUG)]
    pub fn new(config: AudioDepayloaderConfig) -> Self {
        let decoder = if config.fec {
            Some(create_audio_reed_solomon())
        } else {
            None
        };

        Self {
            current_sequence_number: 0,
            data_packets: Default::default(),
            fec_packets: Default::default(),
            fec_decoder: decoder,
        }
    }

    /// Will try to construct the next sample using the internal buffers
    pub fn poll_sample(&mut self) -> Result<Option<AudioSample>, AudioDepayloaderError> {
        let mut output = None;

        if self
            .data_packets
            .contains_key(&self.current_sequence_number)
        {
            // We've received a packet
            let packet = &self.data_packets[&self.current_sequence_number];

            output = Some(packet.to_sample());
            self.current_sequence_number = self.current_sequence_number.wrapping_add(1);
        }
        // All fec reconstruction is done in handle packet

        // Cleanup old data
        // The minimum sequence number that we need to store for fec reconstruction
        let minimum_sequence_number = self
            .current_sequence_number
            .saturating_sub(RTP_AUDIO_DATA_SHARDS as u16);

        self.data_packets
            .retain(|sequence_number, _| *sequence_number >= minimum_sequence_number);

        while self
            .fec_packets
            .pop_front_if(|packet| packet.header.base_sequence_number < minimum_sequence_number)
            .is_some()
        {}

        Ok(output)
    }

    /// Tries to skip samples until it can find a that can be constructed with the current internal buffers.
    /// This doesn't guarantee that the next call to [AudioDepayloader::poll_sample] returns a sample.
    ///
    /// IMPORTANT: After calling this function [AudioDepayloader::poll_sample] MUST be called to get the lowest packets.
    pub fn try_skip_samples(&mut self) -> Result<(), AudioDepayloaderError> {
        let Some((next_data_sequence_number, _)) = self
            .data_packets
            .iter()
            .find(|(sequence_number, _)| **sequence_number >= self.current_sequence_number)
        else {
            // We can't do anything without at least 2 data packets
            return Ok(());
        };

        self.current_sequence_number = *next_data_sequence_number;

        Ok(())
    }

    fn try_reconstruct_fec_block(
        &mut self,
        base_sequence_number: u16,
    ) -> Result<bool, AudioDepayloaderError> {
        let Some(fec_decoder) = self.fec_decoder.as_mut() else {
            return Ok(false);
        };

        // because the fec packets list is in order of base sequence number the fec packets 1/2 will be ordered
        let mut fec_packets = self
            .fec_packets
            .iter()
            .skip_while(|packet| packet.header.base_sequence_number != base_sequence_number)
            .take_while(|packet| packet.header.base_sequence_number == base_sequence_number);

        let mut fec_shard1 = fec_packets.next();
        let mut fec_shard2 = fec_packets.next();

        if fec_shard1.is_none() {
            // At this point we need at least one fec shard to recover the packet, but we don't have that
            // -> It's currently impossible to reconstruct
            return Ok(false);
        }

        // At this point we have at least one fec packet
        // -> this won't panic
        #[allow(clippy::unwrap_used)]
        let some_fec_shard = fec_shard1.unwrap();
        let base_sequence_number = some_fec_shard.header.base_sequence_number;
        let base_timestamp = some_fec_shard.header.base_timestamp;

        if let Some(fec_packet) = fec_shard1
            && fec_packet.header.fec_shard_index > 0
        {
            // We need to swap the first with the second packet because the shard index doesn't match
            fec_shard2 = fec_shard1;
            fec_shard1 = None;
        }

        let fec_shards = fec_shard1.is_some() as usize + fec_shard2.is_some() as usize;

        // -- Check data shards
        let data_shard1 = self.data_packets.get(&base_sequence_number);
        let data_shard2 = self.data_packets.get(&(base_sequence_number + 1));
        let data_shard3 = self.data_packets.get(&(base_sequence_number + 2));
        let data_shard4 = self.data_packets.get(&(base_sequence_number + 3));

        let data_shards = data_shard1.is_some() as usize
            + data_shard2.is_some() as usize
            + data_shard3.is_some() as usize
            + data_shard4.is_some() as usize;

        let total_shards = data_shards + fec_shards;

        if total_shards < RTP_AUDIO_DATA_SHARDS {
            // We currently cannot reconstruct using fec because we don't have enough shards
            return Ok(false);
        }

        // No need to reconstruct data, we already have all shards
        if data_shards == RTP_AUDIO_DATA_SHARDS {
            return Ok(true);
        }

        // TODO: make this more efficient without cloning
        let mut shards = [
            data_shard1.map(|shard| shard.payload.clone()),
            data_shard2.map(|shard| shard.payload.clone()),
            data_shard3.map(|shard| shard.payload.clone()),
            data_shard4.map(|shard| shard.payload.clone()),
            fec_shard1.map(|shard| shard.payload.clone()),
            fec_shard2.map(|shard| shard.payload.clone()),
        ];

        fec_decoder.reconstruct_data(&mut shards)?;

        // The timestamp increment for one audio packet
        // This won't panic because we need at least 2 data packets, excluding the first one we MUST get 1 packet
        let some_data_packets = [data_shard2, data_shard3, data_shard3];
        #[allow(clippy::unwrap_used)]
        let (some_shard_index, some_data_packet) = some_data_packets
            .iter()
            .enumerate()
            .flat_map(|(shard_index, packet)| packet.as_ref().map(|packet| (shard_index, packet)))
            .next()
            .unwrap();
        // This won't panic because sequence_number must be bigger than base_sequence_number
        #[allow(clippy::unwrap_used)]
        let timestamp_increment = some_data_packet
            .timestamp
            .saturating_sub(base_timestamp)
            // We need this one because previously we didn't include the first data shard
            .checked_div(some_shard_index as u32 + 1)
            .unwrap();

        // Store the data shards back into the lists
        for (shard_index, data_shard) in shards[0..RTP_AUDIO_DATA_SHARDS].iter_mut().enumerate() {
            self.data_packets
                .entry(base_sequence_number + shard_index as u16)
                .or_insert_with(|| DataPacket {
                    timestamp: base_timestamp + (timestamp_increment * shard_index as u32),
                    // We just reconstructed all data shards
                    // -> this won't panic
                    #[allow(clippy::unwrap_used)]
                    payload: data_shard.take().unwrap(),
                });
        }

        Ok(true)
    }

    /// Will insert this packet into internal buffers
    pub fn handle_packet(&mut self, packet: &[u8]) -> Result<(), AudioDepayloaderError> {
        if packet.len() < RtpAudioHeader::SIZE {
            // TODO: should we error or just ignore, or warn?
            return Err(AudioDepayloaderError::BufferTooSmall);
        }

        // We checked the size beforehand
        #[allow(clippy::unwrap_used)]
        let rtp_header = RtpAudioHeader::deserialize(
            packet[0..RtpAudioHeader::SIZE]
                .as_array::<{ RtpAudioHeader::SIZE }>()
                .unwrap(),
        );

        if rtp_header.header != RTP_AUDIO_HEADER {
            warn!("Received packet on audio port without standard audio header!");
        }

        if rtp_header.packet_type == INVALID_OPUS_HEADER {
            // TODO: warn?
            return Ok(());
        }

        if rtp_header.packet_type == RTP_PAYLOAD_TYPE_AUDIO {
            let data = &packet[RtpAudioHeader::SIZE..];

            if rtp_header.sequence_number < self.current_sequence_number {
                // Drop the packet because it is too old
                return Ok(());
            }

            let packet = DataPacket {
                timestamp: rtp_header.timestamp,
                payload: data.to_vec(),
            };

            // Insert it into the queue
            self.data_packets.insert(rtp_header.sequence_number, packet);

            // Maybe this is a late data packet -> try fec recovery
            let fec_packet = self.fec_packets.iter().find(|packet| {
                let sequence_number_range = packet.header.base_sequence_number
                    ..(packet.header.base_sequence_number + RTP_AUDIO_DATA_SHARDS as u16);

                sequence_number_range.contains(&rtp_header.sequence_number)
            });
            if let Some(packet) = fec_packet {
                self.try_reconstruct_fec_block(packet.header.base_sequence_number)?;
            }
        } else if rtp_header.packet_type == RTP_PAYLOAD_TYPE_AUDIO_FEC {
            if self.fec_decoder.is_none() {
                warn!("Received audio fec packet even though fec was disabled");
                // fec disabled
                return Ok(());
            }

            if packet.len() < RtpAudioHeader::SIZE + AudioFecHeader::SIZE {
                // TODO: should we error or just ignore, or warn?
                return Err(AudioDepayloaderError::BufferTooSmall);
            }

            // We checked the size beforehand
            #[allow(clippy::unwrap_used)]
            let fec_header = AudioFecHeader::deserialize(
                packet[RtpAudioHeader::SIZE..(RtpAudioHeader::SIZE + AudioFecHeader::SIZE)]
                    .as_array::<{ AudioFecHeader::SIZE }>()
                    .unwrap(),
            );

            let data = &packet[RtpAudioHeader::SIZE + AudioFecHeader::SIZE..];

            let base_sequence_number = fec_header.base_sequence_number;
            if self.current_sequence_number
                > (base_sequence_number.saturating_add(RTP_AUDIO_TOTAL_SHARDS as u16))
            {
                // Drop the packet because it is too old
                return Ok(());
            }

            let packet = FecPacket {
                header: fec_header,
                payload: data.to_vec(),
            };

            // Insert it into the queue ordered by base_sequence_number and fec_shard_index
            match self.fec_packets.binary_search_by(|packet| {
                packet
                    .header
                    .base_sequence_number
                    .cmp(&base_sequence_number)
            }) {
                Ok(index) => {
                    // Element already in vec, look at shard index for correct insertion
                    let other_fec_shard_index = self.fec_packets[index].header.fec_shard_index;

                    if fec_header.fec_shard_index > other_fec_shard_index {
                        self.fec_packets.insert(index + 1, packet);
                    } else {
                        self.fec_packets.insert(index, packet);
                    }
                }
                Err(pos) => self.fec_packets.insert(pos, packet),
            };

            // Try to reconstruct data packets using fec
            self.try_reconstruct_fec_block(base_sequence_number)?;
        } else {
            // TODO: warn?
        }

        Ok(())
    }
}
