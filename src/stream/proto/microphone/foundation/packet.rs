// TODO: Control Stream Extensions: https://github.com/Yundi339/moonlight-common-c/blob/f59424a9f7ad86f2b6278a4e2b07fb2902d8b090/src/Input.h#L198-L214

/// References:
/// - https://github.com/Yundi339/moonlight-common-c/blob/f59424a9f7ad86f2b6278a4e2b07fb2902d8b090/src/MicrophoneStream.c#L4
pub const FOUNDATION_MIC_IV_LEN: usize = 16;
/// References:
/// - https://github.com/Yundi339/moonlight-common-c/blob/f59424a9f7ad86f2b6278a4e2b07fb2902d8b090/src/MicrophoneStream.c#L5
pub const FOUNDATION_MIC_HEADER_FLAGS: u8 = 0x0;

/// References:
/// - https://github.com/Yundi339/moonlight-common-c/blob/f59424a9f7ad86f2b6278a4e2b07fb2902d8b090/src/Limelight-internal.h#L60
pub const FOUNDATION_MIC_MAGIC: u32 = 0x12345678;

/// References:
/// - https://github.com/Yundi339/moonlight-common-c/blob/f59424a9f7ad86f2b6278a4e2b07fb2902d8b090/src/Limelight-internal.h#L61
pub const FOUNDATION_MIC_PACKET_TYPE_OPUS: u8 = 0x61;

/// References:
/// - https://github.com/Yundi339/moonlight-common-c/blob/f59424a9f7ad86f2b6278a4e2b07fb2902d8b090/src/Limelight-internal.h#L61
pub const FOUNDATION_MAX_MIC_PACKET_SIZE: usize = 1400;

/// References:
/// - Mic PR: https://github.com/Yundi339/moonlight-common-c/blob/f59424a9f7ad86f2b6278a4e2b07fb2902d8b090/src/MicrophoneStream.c#L15-L21
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FoundationMicHeader {
    pub flags: u8,
    pub packet_type: u8,
    pub sequence_number: u16,
    pub timestamp: u32,
    pub ssrc: u32,
}

impl FoundationMicHeader {
    pub const SIZE: usize = 1 + 1 + 2 + 4 + 4;

    pub fn serialize(self, header: &mut [u8; Self::SIZE]) {
        header[0] = self.flags;
        header[1] = self.packet_type;

        header[2..4].copy_from_slice(&self.sequence_number.to_le_bytes());
        header[4..8].copy_from_slice(&self.timestamp.to_le_bytes());
        header[8..12].copy_from_slice(&self.ssrc.to_le_bytes());
    }

    pub fn deserialize(header: &[u8; Self::SIZE]) -> Self {
        let flags = header[0];
        let packet_type = header[1];

        let sequence_number = u16::from_le_bytes([header[2], header[3]]);
        let timestamp = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
        let ssrc = u32::from_le_bytes([header[8], header[9], header[10], header[11]]);

        Self {
            flags,
            packet_type,
            sequence_number,
            timestamp,
            ssrc,
        }
    }
}
