#![allow(clippy::unwrap_used)]
#![allow(unused)]

use std::time::Duration;

use gstreamer::{
    Caps, Element, ElementFactory, Pipeline, Sample, State, glib::BoolError, prelude::*,
};
use gstreamer_app::AppSink;

pub struct GStreamerMic {
    pipeline: Pipeline,
    app_sink: AppSink,
}

impl GStreamerMic {
    pub fn new() -> Result<Self, BoolError> {
        let pipeline = Pipeline::new();

        let src = ElementFactory::make("autoaudiosrc").build()?;

        let convert = ElementFactory::make("audioconvert").build()?;
        let resample = ElementFactory::make("audioresample").build()?;

        let opusenc = ElementFactory::make("opusenc").build()?;

        opusenc.set_property("bitrate", 64000i32);
        opusenc.set_property_from_str("frame-size", "20");

        let app_sink = AppSink::builder()
            .name("opus_output")
            .caps(&Caps::builder("audio/x-opus").build())
            .build();

        pipeline.add_many([&src, &convert, &resample, &opusenc, app_sink.upcast_ref()])?;

        Element::link_many([&src, &convert, &resample, &opusenc, app_sink.upcast_ref()])?;

        Ok(Self { pipeline, app_sink })
    }

    pub fn start(&self) {
        self.pipeline.set_state(State::Playing).unwrap();
    }

    pub fn pull_opus_frame_with_timestamp(&self) -> Option<(Vec<u8>, Duration)> {
        let sample = self.app_sink.pull_sample().ok()?;
        let buffer = sample.buffer()?;

        // Extract bytes
        let map = buffer.map_readable().ok()?;
        let data = map.as_slice().to_vec();

        // Extract timestamp (PTS → Duration)
        let timestamp = buffer
            .pts()
            .map(|t| t.nseconds())
            .map(Duration::from_nanos)?;

        Some((data, timestamp))
    }

    pub fn stop(&self) {
        self.pipeline.set_state(State::Null).unwrap();
    }
}
