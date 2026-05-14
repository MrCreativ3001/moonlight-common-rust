# Moonlight over WebRTC
This documents the protocol that [Moonlight Web](https://github.com/MrCreativ3001/moonlight-web-stream) is using to negotiate a WebRTC session and communicate video, audio and control data for game streaming.

It is extending the [WebRTC WHEP](https://datatracker.ietf.org/doc/html/draft-ietf-wish-whep-01) which is used to setup and manage a multimedia session.

## Query Parameters

When a WHEP player initiates a WebRTC session with the Moonlight host, the following query parameters may be included in the `POST` request URL to configure the stream.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `appid` | `u32` | ✅ | The ID of the application/game the server should launch. |
| `mode` | `string` | ✅ | Requested video mode in the format `{width}x{height}x{fps}` (e.g., `1920x1080x60`). |
| `bitrate` | `u32` | ✅ | Target bitrate of the stream in kilobits per second. The server may adjust this based on network conditions. |
| `hdr` | `0` or `1` | ❌ | Request HDR output. `1` enables HDR, `0` disables. Default is `0`. A server must send an `HDRMode` packet to indicate HDR support with any additional HDR information. |
| `localAudioPlayMode` | `0` or `1` | ❌ | Play audio locally (`1`) or only over the stream (`0`). Default is `1`. |
| `preferredCodec` | `u32` | ❌ | Bitmask of preferred video codecs. |
| `preferredAudio` | `u32` | ❌ | Preferred audio configuration. |
| `supportedCodecs` | `u32` | ❌ | Optional: Overwrites the session-supported codecs for web streams. |
| `hostId` | `u32` | ❌ | Optional: Specify the host machine ID to start the stream. |

## Control Stream
A WHEP player will make a request to the WHEP endpoint with a SDP offer ([Section 4-1](https://datatracker.ietf.org/doc/html/draft-ietf-wish-whep-01#section-4-1)).
Custom headers can be used in that request to indicate support for a control stream.

The Simple and ENet control streams use identical Moonlight control packet payloads.
Only the transport semantics differ.
All packets that are sent over the control stream are using the unencrypted payloads documented on the [Wolf Docs](https://games-on-whales.github.io/wolf/stable/protocols/control-specs.html).

The [Enet Control Stream](#enet-control-stream) should always be preferred over the [Simple Control Stream](#simple-control-stream)

### Simple Control Stream
After a WHEP endpoint has received an SDP offer with the `Link: <urn:moonlight:control>; rel="urn:whep:control"` header a server can add a new data channel with the label `control` that must be reliable and ordered.

### Enet Control Stream
After a WHEP endpoint has received an SDP offer with the `Link: <urn:moonlight:control-enet>; rel="urn:whep:control"` header a server can add a new data channel with the label `control` that must be unreliable and unordered.
Furthermore it must have the [`protocol`](https://developer.mozilla.org/en-US/docs/Web/API/RTCDataChannel/protocol) field set to `enet` to differentiate it from the [Simple Control Stream](#simple-control-stream).

When the Enet control stream is used, Moonlight control packets are transported inside ENet packets over the data channel.

## Microphone
To indicate microphone support a server must add the header `Link: <urn:moonlight:microphone>; rel="urn:whep:microphone"` to the [`OPTIONS`](https://datatracker.ietf.org/doc/html/draft-ietf-wish-whep-01#section-4-11) response for a WHEP player.

If a WHEP endpoint has microphone support, a WHEP player can add a microphone track to it's SDP Offer.
This can either be done by using a new `sendonly` audio transceiver or modifying the existing transceiver to be `recvsend`.

## Server Implementation Details
To reduce latency a server should:
- use the [RTP Header Extension to control Playout Delay](https://webrtc.googlesource.com/src/+/refs/heads/main/docs/native-code/rtp-hdrext/playout-delay/README.md) with both the `min` and `max` being 0 on both the audio and video stream

## Client Implementation Details
To reduce latency a client should:
- set the [`jitterBufferTarget`](https://developer.mozilla.org/en-US/docs/Web/API/RTCRtpReceiver/jitterBufferTarget) to 0 on both the audio and video stream where supported
- set the `playoutDelayHint` on the [RTCRtpReceiver](https://developer.mozilla.org/en-US/docs/Web/API/RTCRtpReceiver) to 0 on both the audio and video stream where supported