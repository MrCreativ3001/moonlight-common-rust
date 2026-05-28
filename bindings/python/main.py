# Build the lib
import build

# Run the lib
from moonlight_common import * 

import time

class MyLogger(Logger):
    def log(self, level, text):
        print(f"{level.name}: {text}")

set_logger(logger=MyLogger(),filter=LogLevel.DEBUG)

def audio():
    audio_stream = AudioStream(
        time.time_ns(),
        AudioStreamConfig(
            addr="192.168.178.119:8080",
            opus_config=OpusMultistreamConfig(
                sample_rate=48000,
                channel_count=0,
                streams=0,
                coupled_streams=0,
                samples_per_frame=960,
                mapping=bytes([0,0,0,0,0,0,0,0]),
            ),
            fec=True,
            sunshine_encryption=SunshineEncryption(aes_key=bytes([0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15]),aes_iv=10),
            sunshine_ping=bytes([0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15]),
        )
    )

    print(audio_stream)

    while True:
        output = audio_stream.poll_output()

        print(output)

        if output.is_timeout():
            wait_until = output[0]
            wait_us = max(0, wait_until - time.time_ns())
            time.sleep(wait_us / 1_000_000_000)

            audio_stream.handle_input(AudioStreamInput.TIMEOUT(time.time_ns()))

def video():
    video_stream = VideoStream(
        time.time_ns(),
        VideoStreamConfig(
            packet_size=2048,
            format=VideoFormat.H264,
            server_version=ServerVersion(major=7,minor=0,patch=0,sunshine_identifier=-1,server_type=moonlight_common.ServerType.SUNSHINE),
            fps=60,
            sunshine_ping=None,
            sunshine_encryption=None,
        )
    )

    print(video_stream)

    while True:
        output = video_stream.poll_output()

        print(output)

        if output.is_timeout():
            wait_until = output[0]
            wait_us = max(0, wait_until - time.time_ns())
            time.sleep(wait_us / 1_000_000_000)

            video_stream.handle_input(VideoStreamInput.TIMEOUT(time.time_ns()))

def control():
    control_stream = ControlStream(
        time.time_ns(),
        ControlStreamConfig(
            server_version=ServerVersion(major=7,minor=0,patch=0,sunshine_identifier=-1,server_type=ServerType.SUNSHINE),
            addr="192.168.178.119:8080",
            sunshine_connect_data=None,
            encryption=None,
        )
    )

    print(control_stream)

    while True:
        output = control_stream.poll_output()

        print(output)

        if output.is_timeout():
            wait_until = output[0]
            wait_us = max(0, wait_until - time.time_ns())
            time.sleep(wait_us / 1_000_000_000)

            control_stream.handle_input(ControlStreamInput.TIMEOUT(time.time_ns()))

control()