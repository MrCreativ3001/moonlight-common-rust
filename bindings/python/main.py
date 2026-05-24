# Build the lib
import build

# Run the lib
import moonlight_common

audio_config = moonlight_common.AudioStreamConfig(opus_config=[], fec=True, sunshine_ping=[], sunshine_encryption=None)

print(audio_config)