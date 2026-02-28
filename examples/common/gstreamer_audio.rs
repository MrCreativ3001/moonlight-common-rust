#![allow(clippy::unwrap_used)]
#![allow(unused)]

use gstreamer::{
    Array, Buffer, Caps, ClockTime, Element, ElementFactory, Format, Pipeline, Sample, State,
    glib::{
        BoolError, SendValue,
        value::{ToSendValue, ToValue},
    },
    prelude::{ElementExt, GstBinExtManual},
};
use gstreamer_app::AppSrc;
use moonlight_common::stream::{
    audio::{AudioConfig, AudioDecoder, OpusMultistreamConfig},
    proto::audio::depayloader::AudioSample,
};

pub struct GStreamerAudioDecoder {
    pipeline: Pipeline,
    app_src: AppSrc,
    sample_rate: u32,
    samples_per_frame: u32,
}

impl GStreamerAudioDecoder {
    pub fn new() -> Result<Self, BoolError> {
        // Create a pipeline for audio
        let pipeline = Pipeline::with_name("audio");

        // Create an app source where we'll give the received opus samples into
        let app_src = AppSrc::builder().name("raw opus input").build();
        app_src.set_is_live(true);
        app_src.set_format(Format::Time);
        app_src.set_do_timestamp(true);

        // Opus pipeline that'll convert our opus samples into audio
        let opus_dec = ElementFactory::make_with_name("opusdec", None)?;
        let audio_convert = ElementFactory::make_with_name("audioconvert", None)?;
        let audio_resample = ElementFactory::make_with_name("audioresample", None)?;

        let sink = ElementFactory::make_with_name("autoaudiosink", None)?;

        pipeline
            .add_many([
                app_src.as_ref(),
                &opus_dec,
                &audio_convert,
                &audio_resample,
                &sink,
            ])
            .unwrap();

        Element::link_many([
            app_src.as_ref(),
            &opus_dec,
            &audio_convert,
            &audio_resample,
            &sink,
        ])
        .unwrap();

        Ok(Self {
            pipeline,
            app_src,
            sample_rate: 0,
            samples_per_frame: 0,
        })
    }
}

impl AudioDecoder for GStreamerAudioDecoder {
    fn setup(&mut self, audio_config: AudioConfig, stream_config: OpusMultistreamConfig) -> i32 {
        // Set Capabilities of the opus audio source
        let mapping_slice = &stream_config.mapping[..stream_config.channel_count as usize];

        let caps = Caps::builder("audio/x-opus")
            .field("rate", stream_config.sample_rate as i32)
            .field("channels", stream_config.channel_count as i32)
            .field("stream-count", stream_config.streams as i32)
            .field("coupled-count", stream_config.coupled_streams as i32)
            .field("channel-mapping-family", 1i32)
            .field(
                "channel-mapping",
                Array::from_iter(mapping_slice.iter().map(|x| x.to_send_value())),
            )
            .build();

        self.app_src.set_caps(Some(&caps));

        // Remember sample duration and timestamp conversion
        self.sample_rate = stream_config.sample_rate;
        self.samples_per_frame = stream_config.samples_per_frame;

        0
    }

    fn start(&mut self) {
        // Start the pipeline
        self.pipeline.set_state(State::Playing).unwrap();
    }

    fn decode_and_play_sample(&mut self, sample: AudioSample) {
        let mut buffer = Buffer::from_slice(sample.buffer);

        let pts_ns = (sample.timestamp as u64 * 1_000_000_000) / self.sample_rate as u64;
        let duration_ns = (self.samples_per_frame as u64 * 1_000_000_000) / self.sample_rate as u64;

        let pts = ClockTime::from_nseconds(pts_ns);
        let duration = ClockTime::from_nseconds(duration_ns);

        {
            let buffer_ref = buffer.get_mut().unwrap();
            // buffer_ref.set_pts(Some(pts));
            // buffer_ref.set_duration(Some(duration));
        }

        self.app_src
            .push_buffer(buffer)
            .expect("Failed to push buffer");
    }

    fn stop(&mut self) {
        self.pipeline.set_state(State::Null).unwrap();
    }

    fn config(&self) -> AudioConfig {
        AudioConfig::STEREO
    }
}
