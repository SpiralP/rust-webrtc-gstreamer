# rust-webrtc-gstreamer

Hosts a low-latency WebRTC video server that takes an input source from normal tools like OBS.

## Building

- [Node.js](https://nodejs.org/en/) with NPM

- [GStreamer](https://gstreamer.freedesktop.org/download/) **v1.18+** development installer and runtime installer

`pkg-config` should be able to find `gstreamer-1.0`, `gstreamer-sdp-1.0`, and similar plugins

### macOS

brew doesn't come with gstreamer-sdp-1.0, so you have to use the main download (which puts things in a weird `/Library/Frameworks/GStreamer.framework/Versions/Current` location, make sure to set `PKG_CONFIG_PATH` to the `lib/pkgconfig` directory)

### Linux

take a look at [my Dockerfile](.devcontainer\Dockerfile), debian:testing or ubuntu 20.10 (groovy) have gstreamer 1.18

## Install

```
cargo install --git https://github.com/SpiralP/rust-webrtc-gstreamer.git
```

## Input

strict 2-stream tcp mpeg-ts:

- h264
- aac (2 channels)

---

[OBS](https://obsproject.com/) with tcp url like `tcp://127.0.0.1:1935/`

---

[FFmpeg](https://ffmpeg.org/)

```
ffmpeg -re -i a.mp4 -vcodec libx264 -acodec aac -ac 2 -f mpegts tcp://127.0.0.1:1935/
```

## Reference

https://github.com/centricular/gstwebrtc-demos/blob/master/multiparty-sendrecv/gst-rust/src/main.rs#L312

http://www.francescpinyol.cat/gstreamer.html

## Weirdness

to fix null line in sdp `a=fingerprint:sha-256 (null)`:

- `export OPENSSL_CONF=""`
