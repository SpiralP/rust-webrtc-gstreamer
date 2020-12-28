# rust-webrtc-gstreamer

https://github.com/centricular/gstwebrtc-demos/blob/master/multiparty-sendrecv/gst-rust/src/main.rs#L312

http://www.francescpinyol.cat/gstreamer.html

```
export GST_DEBUG="webrtc*:9"

to fix "a=fingerprint:sha-256 (null)"
export OPENSSL_CONF=""
// a=candidate:1 1 UDP 2147483647 123.123.123.123 50007 typ host


ffmpeg -re -i a.mp4 -vcodec libx264 -acodec aac -ac 2 -f mpegts tcp://127.0.0.1:1935/

```
