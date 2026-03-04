use crate::stream::proto::rtsp::moonlight::SunshinePing;

// TODO: test the serialize and deserialize implementations!
// TODO: move some of these packets into their respective folders / modules with a packet.rs

#[derive(Debug)]
pub struct SunshinePingPacket {
    pub payload: SunshinePing,
    pub sequence_number: u32,
}

impl SunshinePingPacket {
    pub fn deserialize(data: &[u8; 20]) -> Self {
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

    pub fn serialize(&self, data: &mut [u8; 20]) {
        data[0..16].copy_from_slice(&self.payload);
        data[16..20].copy_from_slice(&self.sequence_number.to_be_bytes());
    }
}
