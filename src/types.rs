use serde_derive::{Deserialize, Serialize};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
pub struct Args {
    #[structopt(short, long, default_value = "127.0.0.1:2222")]
    pub server: String,
}

// JSON messages we communicate with
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JsonMsg {
    Sdp(String),
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
