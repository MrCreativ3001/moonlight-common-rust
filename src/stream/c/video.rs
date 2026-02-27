use std::{
    ffi::c_void,
    ops::Deref,
    os::raw::c_int,
    ptr::null_mut,
    slice,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender, channel},
    },
    time::Duration,
};

use moonlight_common_sys::limelight::{
    _DECODER_RENDERER_CALLBACKS, LiCompleteVideoFrame, LiPollNextVideoFrame,
    LiWaitForNextVideoFrame, PDECODE_UNIT, VIDEO_FRAME_HANDLE,
};
use num::FromPrimitive;
use thiserror::Error;
use tracing::debug;

use crate::stream::{
    ColorSpace, SupportedVideoFormats,
    c::bindings::Capabilities,
    video::{
        DecodeResult, VideoCapabilities, VideoDecodeUnit, VideoDecoder, VideoFormat, VideoSetup,
    },
};

static GLOBAL_VIDEO_DECODER: Mutex<Option<Box<dyn VideoDecoder + Send + 'static>>> =
    Mutex::new(None);

fn global_decoder<R>(f: impl FnOnce(&mut dyn VideoDecoder) -> R) -> R {
    let lock = GLOBAL_VIDEO_DECODER.lock();
    let mut lock = lock.expect("global video decoder");

    let decoder = lock.as_mut().expect("global video decoder");
    f(decoder.as_mut())
}

pub(crate) fn set_global(decoder: impl VideoDecoder + Send + 'static) {
    let mut global_video_decoder = GLOBAL_VIDEO_DECODER
        .lock()
        .expect("global video decoder lock");

    *global_video_decoder = Some(Box::new(decoder));
}
pub(crate) fn clear_global() {
    let mut decoder = GLOBAL_VIDEO_DECODER.lock().expect("global video decoder");

    *decoder = None;
}

#[allow(non_snake_case)]
unsafe extern "C" fn setup(
    videoFormat: c_int,
    width: c_int,
    height: c_int,
    redrawRate: c_int,
    _context: *mut c_void,
    _drFlags: c_int,
) -> c_int {
    global_decoder(|decoder| {
        let setup = VideoSetup {
            format: VideoFormat::from_i32(videoFormat).expect("invalid video format"),
            width: width as u32,
            height: height as u32,
            redraw_rate: redrawRate as u32,
        };

        decoder.setup(setup)
    })
}
unsafe extern "C" fn start() {
    global_decoder(|decoder| {
        decoder.start();
    })
}

static BUFFER: Mutex<Vec<&'static [u8]>> = Mutex::new(Vec::new());

unsafe extern "C" fn submit_decode_unit(decode_unit: PDECODE_UNIT) -> c_int {
    // # Safety
    // This buffer is always cleared after (or before use when poisened)
    // -> The data will only be able to be here this call, so 'static is just to get around compiler
    let mut buffers = BUFFER.lock().unwrap_or_else(|buf| {
        let mut buf = buf.into_inner();
        buf.clear();
        buf
    });

    let unit = unsafe { convert_decode_unit(decode_unit, &mut buffers) };

    let result = global_decoder(|decoder| decoder.submit_decode_unit(unit) as i32);

    buffers.clear();

    result
}
/// Converts the cpp decode unit into the rust one
unsafe fn convert_decode_unit<'a>(
    decode_unit: PDECODE_UNIT,
    buffers: &'a mut Vec<&'static [u8]>,
) -> VideoDecodeUnit<'a> {
    buffers.clear();

    let raw = unsafe { *decode_unit };

    let mut next_element_ptr = raw.bufferList;
    while !next_element_ptr.is_null() {
        unsafe {
            let element_raw = *next_element_ptr;

            let new_element =
                slice::from_raw_parts(element_raw.data as *const u8, element_raw.length as usize);
            buffers.push(new_element);

            next_element_ptr = element_raw.next;
        }
    }

    VideoDecodeUnit {
        frame_number: raw.frameNumber,
        frame_processing_latency: if raw.frameHostProcessingLatency == 0 {
            None
        } else {
            Some(Duration::from_millis(
                (raw.frameHostProcessingLatency / 10) as u64,
            ))
        },
        timestamp: raw.rtpTimestamp,
        color_space: ColorSpace::from_u8(raw.colorspace).expect("valid Colorspace"),
        hdr_active: raw.hdrActive,
        buffers,
    }
}

unsafe extern "C" fn stop() {
    global_decoder(|decoder| {
        decoder.stop();
    })
}

unsafe extern "C" fn cleanup() {
    clear_global();
}

pub(crate) unsafe fn raw_callbacks() -> _DECODER_RENDERER_CALLBACKS {
    let video_capabilities = global_decoder(|decoder| decoder.capabilities());

    let mut capabilities = Capabilities::empty();
    if video_capabilities.pull_renderer {
        capabilities |= Capabilities::PULL_RENDERER;
    }
    if video_capabilities.reference_frame_invalidation_h264 {
        capabilities |= Capabilities::REFERENCE_FRAME_INVALIDATION_AVC;
    }
    if video_capabilities.reference_frame_invalidation_h265 {
        capabilities |= Capabilities::REFERENCE_FRAME_INVALIDATION_HEVC;
    }
    if video_capabilities.reference_frame_invalidation_av1 {
        capabilities |= Capabilities::REFERENCE_FRAME_INVALIDATION_AV1;
    }

    _DECODER_RENDERER_CALLBACKS {
        setup: Some(setup),
        start: Some(start),
        stop: Some(stop),
        cleanup: Some(cleanup),
        submitDecodeUnit: if capabilities.contains(Capabilities::PULL_RENDERER) {
            None
        } else {
            Some(submit_decode_unit)
        },
        capabilities: capabilities.bits() as i32,
    }
}

pub struct PullVideoDecoder {
    setup_sender: Sender<VideoSetup>,
    setup_code_receiver: Receiver<i32>,
    active: Arc<AtomicBool>,
    supported_formats: SupportedVideoFormats,
}

pub const ML_PULL_RENDERER_ERROR: i32 = -100001;

impl VideoDecoder for PullVideoDecoder {
    fn setup(&mut self, setup: VideoSetup) -> i32 {
        if self.setup_sender.send(setup).is_err() {
            return ML_PULL_RENDERER_ERROR;
        }

        match self.setup_code_receiver.recv() {
            Ok(value) => value,
            Err(_) => ML_PULL_RENDERER_ERROR,
        }
    }

    fn start(&mut self) {
        self.active.store(true, Ordering::Release);
    }

    fn stop(&mut self) {
        self.active.store(false, Ordering::Release);
    }

    fn submit_decode_unit(&mut self, _unit: VideoDecodeUnit<'_>) -> DecodeResult {
        unreachable!()
    }

    fn supported_formats(&self) -> SupportedVideoFormats {
        self.supported_formats
    }

    fn capabilities(&self) -> VideoCapabilities {
        VideoCapabilities {
            pull_renderer: true,
            ..Default::default()
        }
    }
}

#[derive(Debug, Error)]
pub enum VideoPullResult {
    #[error("no video decode unit present")]
    ValueNotPresent,
    #[error("this video renderer is not active")]
    NotActive,
    #[error("cannot finish the video renderer setup")]
    CannotFinishSetup,
    #[error("the setup success was already sent")]
    SetupSuccessAlreadySent,
}

pub struct PullVideoManager {
    setup_receiver: Receiver<VideoSetup>,
    setup: Option<VideoSetup>,
    setup_code_sender: Option<Sender<i32>>,
    active: Arc<AtomicBool>,
    buffers: Vec<&'static [u8]>,
}

impl PullVideoManager {
    pub fn new(supported_formats: SupportedVideoFormats) -> (PullVideoDecoder, PullVideoManager) {
        let active = Arc::new(AtomicBool::new(false));

        let (setup_sender, setup_receiver) = channel();
        let (setup_code_sender, setup_code_receiver) = channel();

        (
            PullVideoDecoder {
                active: active.clone(),
                setup_sender,
                setup_code_receiver,
                supported_formats,
            },
            PullVideoManager {
                active,
                setup_receiver,
                setup: None,
                setup_code_sender: Some(setup_code_sender),
                buffers: Vec::new(),
            },
        )
    }

    fn check_active(&self) -> Result<(), VideoPullResult> {
        if self.active.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(VideoPullResult::NotActive)
        }
    }

    /// Note: Send setup result must be called after a successful pull
    pub fn poll_setup(&mut self) -> Result<VideoSetup, VideoPullResult> {
        match self.setup_receiver.try_recv() {
            Ok(setup) => {
                self.setup = Some(setup);

                Ok(setup)
            }
            Err(err) => {
                debug!("failed to receive video setup: {err}");
                Err(VideoPullResult::NotActive)
            }
        }
    }
    /// Note: Send setup result must be called after this
    pub fn wait_for_setup(&mut self) -> Result<VideoSetup, VideoPullResult> {
        if let Some(setup) = self.setup.as_ref() {
            return Ok(*setup);
        }

        match self.setup_receiver.recv() {
            Ok(setup) => {
                self.setup = Some(setup);

                Ok(setup)
            }
            Err(err) => {
                debug!("failed to receive video setup: {err}");
                Err(VideoPullResult::NotActive)
            }
        }
    }

    pub fn send_setup_result(&mut self, code: i32) -> Result<(), VideoPullResult> {
        let Some(setup_code_sender) = self.setup_code_sender.take() else {
            return Err(VideoPullResult::SetupSuccessAlreadySent);
        };

        if let Err(err) = setup_code_sender.send(code) {
            self.setup_code_sender = Some(setup_code_sender);

            debug!("failed to send video setup success: {err}");
            return Err(VideoPullResult::CannotFinishSetup);
        }

        Ok(())
    }

    pub fn poll_next_video_frame<'a>(
        &'a mut self,
    ) -> Result<PullVideoDecodeUnit<'a>, VideoPullResult> {
        self.check_active()?;

        unsafe {
            let mut frame_handle: VIDEO_FRAME_HANDLE = null_mut();
            let mut decode_unit: PDECODE_UNIT = null_mut();

            if !LiPollNextVideoFrame(&mut frame_handle, &mut decode_unit) {
                return Err(VideoPullResult::ValueNotPresent);
            }

            Ok(self.handle_next_video_frame(frame_handle, decode_unit))
        }
    }
    pub fn wait_for_next_video_frame<'a>(
        &'a mut self,
    ) -> Result<PullVideoDecodeUnit<'a>, VideoPullResult> {
        self.check_active()?;

        unsafe {
            let mut frame_handle: VIDEO_FRAME_HANDLE = null_mut();
            let mut decode_unit: PDECODE_UNIT = null_mut();

            if !LiWaitForNextVideoFrame(&mut frame_handle, &mut decode_unit) {
                return Err(VideoPullResult::ValueNotPresent);
            }

            Ok(self.handle_next_video_frame(frame_handle, decode_unit))
        }
    }

    /// # Safety
    /// - The PullVideoDecodeUnit will call LiCompleteVideoFrame after it's complete
    /// - No calls to NextVideoFrame are allowed until it is dropped because of &'a mut reference completing the cycle of a video frame
    unsafe fn handle_next_video_frame<'a>(
        &'a mut self,
        frame_handle: VIDEO_FRAME_HANDLE,
        decode_unit: PDECODE_UNIT,
    ) -> PullVideoDecodeUnit<'a> {
        unsafe {
            let decode_unit = convert_decode_unit(decode_unit, &mut self.buffers);

            PullVideoDecodeUnit {
                frame_handle,
                result: DecodeResult::default(),
                decode_unit,
            }
        }
    }
}

pub struct PullVideoDecodeUnit<'a> {
    frame_handle: VIDEO_FRAME_HANDLE,
    decode_unit: VideoDecodeUnit<'a>,
    result: DecodeResult,
}

impl<'a> PullVideoDecodeUnit<'a> {
    pub fn set_result(&mut self, result: DecodeResult) {
        self.result = result;
    }
}

impl<'a> Deref for PullVideoDecodeUnit<'a> {
    type Target = VideoDecodeUnit<'a>;

    fn deref(&self) -> &Self::Target {
        &self.decode_unit
    }
}

impl<'a> Drop for PullVideoDecodeUnit<'a> {
    fn drop(&mut self) {
        unsafe {
            LiCompleteVideoFrame(self.frame_handle, self.result as i32);
        }
    }
}
