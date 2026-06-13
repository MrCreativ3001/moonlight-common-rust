# Moonlight over WebRTC
This documents the protocol that [Moonlight Web](https://github.com/MrCreativ3001/moonlight-web-stream) is using to negotiate a WebRTC session and communicate video, audio and control data for game streaming.

It is extending the [WebRTC WHEP Specification](https://datatracker.ietf.org/doc/html/draft-ietf-wish-whep-01) which is used to setup and manage a multimedia session.

Any additional video, audio or data channels that are not recognized by the WHEP player or WHEP endpoint should be ignored.

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
| `a=x-moonlight-control` | `"simple"` or `"enet"` | ❌ | It's allowed to add this attribute multiple times. See [Control Stream](#control-stream) |

### Answer Attributes

| Attribute | Type | Required | Description |
|---|---|---|---|
| `a=x-moonlight-app-name` | `string` | ❌ | The name of the app that was started. |
| `a=x-moonlight-microphone` | `0` or `1` | ❌ | See [Microphone](#microphone) |

## Control Stream
A WHEP player will make a request to the WHEP endpoint with a SDP offer ([Section 4-1](https://datatracker.ietf.org/doc/html/draft-ietf-wish-whep-01#section-4-1)).
Control stream support is negotiated entirely through SDP attributes.

The Simple and ENet control streams use identical Moonlight control packet payloads.
Only the transport semantics differ.
All packets that are sent over the control stream are using the unencrypted payloads documented on the [Wolf Docs](https://games-on-whales.github.io/wolf/stable/protocols/control-specs.html).

### Control Stream Negotiation

The client indicates supported control stream types using:

```sdp
a=x-moonlight-control:simple
a=x-moonlight-control:enet
```

A server selects one mode in the SDP answer.

If multiple modes are offered, `enet` should be preferred when supported.

## Simple Control Stream

When the negotiated control mode is `simple`, the server creates a WebRTC data channel with:

- label: `moonlight.control`
- ordered: `true`
- reliable delivery enabled

## ENet Control Stream

When the negotiated control mode is `enet`, the server creates a WebRTC data channel with:

- label: `moonlight.control`
- unreliable delivery (ordered: false, maxRetransmits: 0)
- protocol: `enet`

When the Enet control stream is used, Moonlight control packets are transported inside ENet packets over the data channel.

Multiple enet peers are allowed to connect over the single WebRTC data channel.
If no client peer is connected to the server peer, the server should stop sending video and audio because the stream was unfocused.

## Microphone
A WHEP player can add a microphone track to it's SDP Offer.
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