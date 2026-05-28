# Build the lib
import build

# Run the lib
import moonlight_common

import time

class MyLogger(moonlight_common.Logger):
    def log(self, level, text):
        print(f"{level.name}: {text}")

moonlight_common.set_logger(logger=MyLogger(),filter=moonlight_common.LogLevel.DEBUG)

def audio():
    audio_stream = moonlight_common.AudioStream(
        0,
        moonlight_common.AudioStreamConfig(
            addr="192.168.178.119:8080",
            opus_config=moonlight_common.OpusMultistreamConfig(
                sample_rate=48000,
                channel_count=0,
                streams=0,
                coupled_streams=0,
                samples_per_frame=960,
                mapping=bytes([0,0,0,0,0,0,0,0]),
            ),
            fec=True,
            sunshine_encryption=moonlight_common.SunshineEncryption(aes_key=bytes([0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15]),aes_iv=10),
            sunshine_ping=bytes([0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15]),
        )
    )

    print(audio_stream)

    while True:
        output = audio_stream.poll_output()

        if output.is_timeout():
            wait_until = output[0]
            wait_us = max(0, wait_until - time.time_ns())
            time.sleep(wait_us / 1_000_000_000)

            audio_stream.handle_input(moonlight_common.AudioStreamInput.TIMEOUT(time.time_ns()))

def video():
    video_stream = moonlight_common.VideoStream(
        0,
        moonlight_common.VideoStreamConfig(
            packet_size=2048,
            format=moonlight_common.VideoFormat.H264,
            server_version=moonlight_common.ServerVersion(major=7,minor=0,patch=0,sunshine_identifier=-1,server_type=moonlight_common.ServerType.SUNSHINE),
            fps=60,
            sunshine_ping=None,
            sunshine_encryption=None,
        )
    )

    print(video_stream)

    while True:
        output = video_stream.poll_output()

        if output.is_timeout():
            wait_until = output[0]
            wait_us = max(0, wait_until - time.time_ns())
            time.sleep(wait_us / 1_000_000_000)

            video_stream.handle_input(moonlight_common.VideoStreamInput.TIMEOUT(time.time_ns()))

video()