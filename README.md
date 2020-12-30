# rust-webrtc-gstreamer

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
