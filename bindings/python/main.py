# Build the lib
import build

# Run the lib
from moonlight_common_bindings import * 

import time

class MyLogger(Logger):
    def log(self, level, text):
        print(f"{level.name}: {text}")

set_logger(logger=MyLogger(),filter=LogLevel.DEBUG)

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
        event = control_stream.poll_event()
        packet_to_send = control_stream.poll_packet()
        wait_until = control_stream.poll_timeout()

        print(event)
        print(packet_to_send)
        print(wait_until)

        wait_us = max(0, wait_until - time.time_ns())
        time.sleep(wait_us / 1_000_000_000)

        control_stream.handle_timeout(time.time_ns())

control()