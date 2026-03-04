use gstreamer::{
    Buffer, BufferFlags, ClockTime, Element, ElementFactory, Format, Pipeline, State,
    glib::{BoolError, object::ObjectExt},
    prelude::{ElementExt, GstBinExtManual},
};
use gstreamer_app::AppSrc;
use moonlight_common::stream::video::{
    DecodeResult, SupportedVideoFormats, VideoDecodeUnit, VideoDecoder, VideoFormat, VideoSetup,
};

pub struct GStreamerVideoDecoder {
    pipeline: Pipeline,
    app_src: AppSrc,
}

impl GStreamerVideoDecoder {
    pub fn new() -> Result<Self, BoolError> {
        // Create a pipeline for audio
        let pipeline = Pipeline::with_name("video");

        // Create an app source where we'll give the received opus samples into
        let app_src = AppSrc::builder().name("raw video input").build();
        app_src.set_is_live(true);
        app_src.set_format(Format::Time);
        app_src.set_do_timestamp(false);
        app_src.set_block(false);
        app_src.set_max_bytes(0);
        app_src.set_min_latency(0);

        // Opus pipeline that'll convert our opus samples into audio
        let video_parse = ElementFactory::make_with_name("h264parse", None)?;
        let video_decode = ElementFactory::make_with_name("avdec_h264", None)?;

        let sink = ElementFactory::make_with_name("autovideosink", None)?;
        sink.set_property("sync", false);

        pipeline
            .add_many([app_src.as_ref(), &video_parse, &video_decode, &sink])
            .unwrap();

        Element::link_many([app_src.as_ref(), &video_parse, &video_decode, &sink]).unwrap();

        Ok(Self { pipeline, app_src })
    }
}

impl VideoDecoder for GStreamerVideoDecoder {
    fn setup(&mut self, setup: VideoSetup) -> i32 {
        if !matches!(setup.format, VideoFormat::H264) {
            // this decoder doesn't support other formats than h264
            return -1;
        }

        0
    }

    fn start(&mut self) {
        // Start pipeline
        self.pipeline.set_state(State::Playing);
    }

    fn submit_decode_unit(&mut self, unit: VideoDecodeUnit<'_>) -> DecodeResult {
        for buffer in unit.buffers {
            let mut gst_buffer = Buffer::with_size(buffer.data.len()).unwrap();
            {
                let buffer_mut = gst_buffer.get_mut().unwrap();

                buffer_mut.copy_from_slice(0, buffer.data).unwrap();

                buffer_mut.set_pts(ClockTime::from_nseconds(unit.timestamp.as_nanos() as u64));
            }
            self.app_src.push_buffer(gst_buffer).unwrap();
        }

        DecodeResult::Ok
    }

    fn stop(&mut self) {
        // Stop pipeline
        self.pipeline.set_state(State::Null);
    }

    fn supported_formats(&self) -> SupportedVideoFormats {
        SupportedVideoFormats::H264
    }
}
