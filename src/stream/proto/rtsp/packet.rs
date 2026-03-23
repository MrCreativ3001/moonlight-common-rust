// https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L92
pub const ENCRYPTED_RTSP_BIT: u32 = 0x80000000;

// https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L94
#[derive(Debug, Clone, Copy)]
pub struct RtspEncryptionHeader {
    pub encrypted: bool,
    pub len: usize,
    pub sequence_number: usize,
    pub tag: [u8; 16],
}

impl RtspEncryptionHeader {
    pub const SIZE: usize = 4 + 4 + 16;

    // https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L100
    pub fn serialize(self, header: &mut [u8; Self::SIZE]) {
        let type_and_length: u32 = self.len as u32
            | if self.encrypted {
                ENCRYPTED_RTSP_BIT
            } else {
                0
            };

        header[0..4].copy_from_slice(&u32::to_be_bytes(type_and_length));
        header[4..8].copy_from_slice(&u32::to_be_bytes(self.sequence_number as u32));
        header[8..24].copy_from_slice(&self.tag);
    }

    // https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L155
    pub fn deserialize(header: &[u8; Self::SIZE]) -> Self {
        let type_and_length = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
        let sequence_number = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);

        let encrypted = (type_and_length & ENCRYPTED_RTSP_BIT) != 0;
        let len = (type_and_length & !ENCRYPTED_RTSP_BIT) as usize;

        let mut tag = [0u8; 16];
        tag.copy_from_slice(&header[8..24]);

        Self {
            encrypted,
            len,
            sequence_number: sequence_number as usize,
            tag,
        }
    }
}
