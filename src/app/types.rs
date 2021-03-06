use serde_derive::{Deserialize, Serialize};
use std::net::SocketAddr;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
pub struct Args {
    /// http/websocket signaling listen address
    #[structopt(long, default_value = "127.0.0.1:8080")]
    pub signal_server: SocketAddr,

    /// tcp mpegts input stream listen address
    #[structopt(long, default_value = "127.0.0.1:1935")]
    pub stream_server: SocketAddr,

    #[structopt(long, default_value = "8")]
    pub cpu_used: i8,

    // 100KiB/s
    /// Target bitrate (in bits/sec)
    #[structopt(long, default_value = "819200")]
    pub video_bitrate: u32,

    // ~16KiB/s
    /// bitrate (in bits/sec)
    #[structopt(long, default_value = "128000")]
    pub audio_bitrate: u32,

    #[structopt(short, long, parse(from_occurrences))]
    pub verbose: u8,

    #[structopt(long)]
    pub udp: bool,
}

// JSON messages we communicate with
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JsonMsg {
    Sdp(String),

    #[serde(rename_all = "camelCase")]
    Ice {
        candidate: String,
        sdp_m_line_index: u32,
    },

    #[serde(rename_all = "camelCase")]
    Stats {
        total_packets_received: usize,
        total_packets_lost: usize,
    },
}

#[test]
fn test_serialize() {
    let sdp = JsonMsg::Sdp("hi".to_string());

    assert_eq!(serde_json::to_string(&sdp).unwrap(), r#"{"sdp":"hi"}"#);

    // let ice = JsonMsg::Ice {
    //     candidate: "hi".to_string(),
    //     sdp_m_line_index: 55,
    // };

    // assert_eq!(
    //     serde_json::to_string(&ice).unwrap(),
    //     r#"{"ice":{"candidate":"hi","sdpMLineIndex":55}}"#
    // );
}

#[test]
fn test_deserialize() {
    // let ice: JsonMsg = serde_json::from_str(
    //     r#"{
    //         "ice": {
    //             "candidate": "hello",
    //             "sdpMLineIndex": 99
    //         }
    //     }"#,
    // )
    // .unwrap();

    // if let JsonMsg::Ice {
    //     candidate,
    //     sdp_m_line_index,
    // } = ice
    // {
    //     assert_eq!(candidate, "hello");
    //     assert_eq!(sdp_m_line_index, 99);
    // } else {
    //     unreachable!();
    // }
}
