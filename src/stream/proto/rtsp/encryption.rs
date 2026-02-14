use crate::stream::proto::{
    MoonlightStreamConfig,
    crypto::{CipherAlgorithm, CryptoContext, CryptoError},
};

// https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L92
pub const ENCRYPTED_RTSP_BIT: u32 = 0x80000000;

pub const RTSP_HEADER_SIZE: usize = 4 + 4 + 16;

// https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L94
#[derive(Clone, Copy)]
pub struct RtspEncryptionHeader {
    pub encrypted: bool,
    pub len: usize,
    pub sequence_number: usize,
    pub tag: [u8; 16],
}

impl RtspEncryptionHeader {
    // https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L100
    pub fn serialize(self, header: &mut [u8; RTSP_HEADER_SIZE]) {
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
    pub fn deserialize(header: &[u8; RTSP_HEADER_SIZE]) -> Self {
        todo!()
    }
}

// https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L100
pub fn encrypt_rtsp_message(
    context: &impl CryptoContext,
    stream: &MoonlightStreamConfig,
    sequence_number: usize,
    message: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let mut iv = [0; 12];
    iv[0..4].copy_from_slice(&u32::to_le_bytes(sequence_number as u32));

    iv[10] = b'C'; // Client originated
    iv[11] = b'R'; // RTSP stream

    let mut data = vec![0; RTSP_HEADER_SIZE + message.len()];

    let mut header = RtspEncryptionHeader {
        encrypted: true,
        len: message.len(),
        sequence_number,
        tag: [0; 16],
    };

    context.encrypt(
        CipherAlgorithm::AesGcm,
        (),
        &stream.remote_input_aes_key,
        &iv,
        &mut header.tag,
        message,
        &mut data[RTSP_HEADER_SIZE..],
    )?;

    #[allow(clippy::unwrap_used)]
    // This won't panic because we're literally using the size to get the slice
    header.serialize(
        data[0..RTSP_HEADER_SIZE]
            .as_mut_array::<RTSP_HEADER_SIZE>()
            .unwrap(),
    );

    Ok(data)
}

// https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L155
pub fn decrypt_rtsp_message() {}
