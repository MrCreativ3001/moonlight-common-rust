use std::{
    ffi::c_void,
    os::raw::{c_char, c_int},
    slice,
    sync::Mutex,
};

use moonlight_common_sys::limelight::{_AUDIO_RENDERER_CALLBACKS, POPUS_MULTISTREAM_CONFIGURATION};

use crate::stream::{
    AudioConfig,
    audio::{AudioDecoder, OpusMultistreamConfig},
    c::bindings::Capabilities,
    proto::audio::depayloader::AudioSample,
};

static GLOBAL_AUDIO_DECODER: Mutex<Option<Box<dyn AudioDecoder + Send + 'static>>> =
    Mutex::new(None);

fn global_decoder<R>(f: impl FnOnce(&mut dyn AudioDecoder) -> R) -> R {
    let lock = GLOBAL_AUDIO_DECODER.lock();
    let mut lock = lock.expect("global audio decoder");

    let decoder = lock.as_mut().expect("global audio decoder");
    f(decoder.as_mut())
}

pub(crate) fn set_global(decoder: impl AudioDecoder + Send + 'static) {
    let mut global_audio_decoder = GLOBAL_AUDIO_DECODER
        .lock()
        .expect("global audio decoder lock");

    *global_audio_decoder = Some(Box::new(decoder));
}
pub(crate) fn clear_global() {
    let mut decoder = GLOBAL_AUDIO_DECODER.lock().expect("global video decoder");

    *decoder = None;
}

#[allow(non_snake_case)]
unsafe extern "C" fn setup(
    audioConfiguration: c_int,
    opusConfig: POPUS_MULTISTREAM_CONFIGURATION,
    _context: *mut c_void,
    _arFlags: c_int,
) -> c_int {
    global_decoder(|decoder| {
        let audio_config =
            AudioConfig::from_raw(audioConfiguration as u32).expect("a valid audio configuration");

        let raw_opus_config = unsafe { *opusConfig };
        let opus_config = OpusMultistreamConfig {
            sample_rate: raw_opus_config.sampleRate as u32,
            channel_count: raw_opus_config.channelCount as u32,
            streams: raw_opus_config.streams as u32,
            coupled_streams: raw_opus_config.coupledStreams as u32,
            samples_per_frame: raw_opus_config.samplesPerFrame as u32,
            mapping: raw_opus_config.mapping,
        };

        decoder.setup(audio_config, opus_config)
    })
}
unsafe extern "C" fn start() {
    global_decoder(|decoder| {
        decoder.start();
    })
}

unsafe extern "C" fn decode_and_play_sample(data: *mut c_char, len: c_int) {
    global_decoder(|decoder| unsafe {
        let data = slice::from_raw_parts(data as *mut u8, len as usize);

        // TODO: how to track the timestamp?
        // TODO: remove clone

        decoder.decode_and_play_sample(AudioSample {
            timestamp: 0,
            buffer: data.to_vec(),
        });
    })
}

unsafe extern "C" fn stop() {
    global_decoder(|decoder| {
        decoder.stop();
    })
}

unsafe extern "C" fn cleanup() {
    clear_global();
}

pub(crate) unsafe fn raw_callbacks() -> _AUDIO_RENDERER_CALLBACKS {
    let capabilities = Capabilities::empty();

    _AUDIO_RENDERER_CALLBACKS {
        init: Some(setup),
        start: Some(start),
        stop: Some(stop),
        cleanup: Some(cleanup),
        decodeAndPlaySample: Some(decode_and_play_sample),
        capabilities: capabilities.bits() as i32,
    }
}
