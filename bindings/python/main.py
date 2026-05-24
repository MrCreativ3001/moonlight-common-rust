# Build the lib
import build

# Run the lib
import moonlight_common
import time

audio_config = moonlight_common.AudioStreamConfig(
    opus_config=moonlight_common.OpusMultistreamConfig(sample_rate=48000, channel_count=2, streams=1, coupled_streams=1, samples_per_frame=960, mapping=bytes([0,1,0,0,0,0,0,0])),
    fec=True,
    sunshine_ping=None,
    sunshine_encryption=None
)

print(audio_config)

audio_stream = moonlight_common.AudioStream(time.time_ns(), audio_config)

while True:
    output = audio_stream.poll_output()
    print(output)

    if output.is_timeout():
        wait_until = output[0]
        wait_us = max(0, wait_until - time.time_ns())
        time.sleep(wait_us / 1_000_000_000)

        audio_stream.handle_input(moonlight_common.AudioStreamInput.TIMEOUT(time.time_ns()))