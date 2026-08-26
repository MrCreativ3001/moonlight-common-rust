# Moonlight over WebRTC [WIP]
This documents the protocol that [Moonlight Web](https://github.com/MrCreativ3001/moonlight-web-stream) is using to negotiate a WebRTC session and communicate video, audio and control data for game streaming.
It is inspired by the [WHIP](https://www.ietf.org/archive/id/draft-ietf-wish-whip-01.html) and [WHEP](https://www.ietf.org/archive/id/draft-murillo-whep-03.html) specification.

## WebRTC Endpoint
The server MUST expose a dedicated HTTP endpoint to handle session negotiations initiated by the client.

### Options
The server can expose the HTTP `OPTIONS` method on the endpoint to return ice servers.
Those can be utilized by the WebRTC client.

Example response headers:
```http
Link: <stun:stun.example.net>;
Link: <turn:turn.example.net?transport=udp>; rel="ice-server"; username="user"; credential: "myPassword"; credential-type: "password";
Link: <turn:turn.example.net?transport=tcp>; rel="ice-server"; username="user"; credential: "myPassword"; credential-type: "password";
Link: <turns:turn.example.net?transport=tcp>; rel="ice-server"; username="user"; credential: "myPassword"; credential-type: "password";
```

### Launching a WebRTC stream
To initiate a stream, the client MUST issue an HTTP `POST` request to the endpoint.

Request Requirements:
- Headers
  - `Content-Type: application/sdp`
- Sdp offer constraints
  - [Offer Attributes](#offer-attributes)
  - `recv` or `recvsend` video and audio transceivers
  - data channel transceiver (`m=application` line)

Response Requirements:
- Headers
  - `Content-Type: application/sdp`
  - `Location: /webrtc/{STREAM_IDENTIFIER}`: The location of the WebRTC stream
- body must contain a valid webrtc answer from the server
- Sdp answer constrains:
  - [Answer Attributes](#answer-attributes)
- Response Code: `201 Created`

### Trickle Ice Candidates
To make negotation quick, trickle ice candidates can be used by the client.
The server must collect all it's ice candidates before responding to the client.
After creating a stream a `PATCH` request can be made to the returned `Location` header in the launch request.

Request Requirements:
- Headers
  - `Content-Type: application/trickle-ice-sdpfrag`
- Content
  - New line seperated ice candidates

Response Requirements:
- Response Code: `204 No Content`

Example request:
```http
PATCH /resource/id HTTP/1.1
Host: example.com
Content-Type: application/trickle-ice-sdpfrag
Content-Length: 433

a=candidate:1387637174 1 udp 2122260223 192.0.2.1 61764 typ host generation 0 ufrag EsAw network-id 1
a=candidate:3471623853 1 udp 2122194687 198.51.100.1 61765 typ host generation 0 ufrag EsAw network-id 2
a=candidate:473322822 1 tcp 1518280447 192.0.2.1 9 typ host tcptype active generation 0 ufrag EsAw network-id 1
a=candidate:2154773085 1 tcp 1518214911 198.51.100.2 9 typ host tcptype active generation 0 ufrag EsAw network-id 2

HTTP/1.1 204 No Content
```

### Stopping a stream
After creating a stream a `DELETE` request can be made to the returned `Location` header in the launch request.
This will immediatly stop the stream.

## SDP Session Attributes

Moonlight-specific configuration is negotiated through custom SDP attributes in the offer/answer exchange.

All custom attributes use the `x-moonlight-*` namespace.

Example:

```sdp
a=x-moonlight-app-id:12345
a=x-moonlight-mode:1920x1080x60
a=x-moonlight-bitrate:20000
a=x-moonlight-hdr:1
```

### Offer Attributes

| Attribute | Type | Required | Description |
|---|---|---|---|
| `a=x-moonlight-app-id` | `u32` | ✅ | The ID of the application/game the server should launch. |
| `a=x-moonlight-mode` | `string` | ✅ | Requested video mode in the format `{width}x{height}x{fps}` (e.g. `1920x1080x60`). |
| `a=x-moonlight-bitrate` | `u32` | ✅ | Target bitrate of the stream in kilobits per second. The server may adjust this based on network conditions. |
| `a=x-moonlight-hdr` | `0` or `1` | ❌ | Request HDR output. `1` enables HDR, `0` disables. Default is `0`. A server must send an `HDRMode` packet to indicate HDR support and provide additional HDR information. |
| `a=x-moonlight-local-audio-play-mode` | `0` or `1` | ❌ | Play audio locally (`1`) or only over the stream (`0`). Default is `1`. |
| `a=x-moonlight-preferred-codec` | `u32` | ❌ | Bitmask of preferred video codecs. |
| `a=x-moonlight-preferred-audio` | `u32` | ❌ | Preferred audio configuration. |
| `a=x-moonlight-host-id` | `u32` | ❌ | Specify the host machine ID to start the stream on. |

### Answer Attributes

| Attribute | Type | Required | Description |
|---|---|---|---|
| `a=x-moonlight-app-name` | `string` | ❌ | The name of the app that was started. |
| `a=x-moonlight-microphone` | `0` or `1` | ❌ | See [Microphone](#microphone) |

## Control Stream
The data channel with the label `moonlight.control` will be added by the server and is used for reliable and ordered control packet transmission.

The client can add other data channels with the wildcard label `moonlight.control.*`.
This can be used for sending unreliable packets.

All packets that are sent over the control stream are using the unencrypted payloads documented on the [Wolf Docs](https://games-on-whales.github.io/wolf/stable/protocols/control-specs.html).

## Microphone
A client can add a microphone track to it's SDP Offer.
This can either be done by using a new `sendonly` audio transceiver or modifying the existing transceiver to be `recvsend`.

The server indicates microphone support in it's answer using:
```sdp
a=x-moonlight-microphone:1
```

## Server Implementation Details
To reduce latency a server should:
- use the [RTP Header Extension to control Playout Delay](https://webrtc.googlesource.com/src/+/refs/heads/main/docs/native-code/rtp-hdrext/playout-delay/README.md) with both the `min` and `max` being 0 on both the audio and video stream

## Client Implementation Details
To reduce latency a client should:
- set the [`jitterBufferTarget`](https://developer.mozilla.org/en-US/docs/Web/API/RTCRtpReceiver/jitterBufferTarget) to 0 on both the audio and video stream where supported
- set the `playoutDelayHint` on the [RTCRtpReceiver](https://developer.mozilla.org/en-US/docs/Web/API/RTCRtpReceiver) to 0 on both the audio and video stream where supported