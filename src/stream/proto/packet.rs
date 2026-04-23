use std::ops::Deref;

// https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/Video.h#L48
pub const SUNSHINE_PING_PAYLOAD_SIZE: usize = 16;

#[derive(Debug, Clone)]
pub struct SunshinePing(pub [u8; SUNSHINE_PING_PAYLOAD_SIZE]);

impl Deref for SunshinePing {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug)]
pub struct SunshinePingPacket {
    pub payload: SunshinePing,
    pub sequence_number: u32,
}

impl SunshinePingPacket {
    pub const SIZE: usize = 20;

    #[allow(unused)]
    pub fn deserialize(data: &[u8; Self::SIZE]) -> Self {
        let mut payload = [0; 16];
        payload.copy_from_slice(&data[0..16]);

        // Won't panic because 20-16=4
        #[allow(clippy::unwrap_used)]
        let sequence_number = u32::from_be_bytes(*data[16..20].as_array::<4>().unwrap());

        Self {
            payload: SunshinePing(payload),
            sequence_number,
        }
    }

    pub fn serialize(&self, data: &mut [u8; Self::SIZE]) {
        data[0..16].copy_from_slice(&self.payload);
        data[16..20].copy_from_slice(&self.sequence_number.to_be_bytes());
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod test {
    use crate::stream::proto::packet::{SunshinePing, SunshinePingPacket};

    #[test]
    fn sunshine_ping_packet() {
        let assert_packet_eq = |expected_packet: SunshinePingPacket, expected_data: [u8; 20]| {
            // Test serialize
            let mut buffer = [0u8; 20];
            expected_packet.serialize(&mut buffer);
            assert_eq!(buffer, expected_data);

            // Test deserialize
            let decoded = SunshinePingPacket::deserialize(&expected_data);
            assert_eq!(decoded.payload.0, expected_packet.payload.0);
            assert_eq!(decoded.sequence_number, expected_packet.sequence_number);
        };

        let packet = SunshinePingPacket {
            payload: SunshinePing([
                0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD,
                0xEE, 0xFF,
            ]),
            sequence_number: 0xAABBCCDD,
        };

        let expected_bytes = [
            // payload (16 bytes)
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD,
            0xEE, 0xFF, // sequence_number (big endian)
            0xAA, 0xBB, 0xCC, 0xDD,
        ];

        assert_packet_eq(packet, expected_bytes);
    }
}
