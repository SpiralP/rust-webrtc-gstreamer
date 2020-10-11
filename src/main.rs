#![recursion_limit = "512"]
// to fix "a=fingerprint:sha-256 (null)"
// export OPENSSL_CONF=""
// a=candidate:1 1 UDP 2147483647 123.123.123.123 50007 typ host

mod app;
mod types;

use self::{app::App, types::Args};
use crate::types::JsonMsg;
use anyhow::bail;
use async_std::{net::TcpListener, prelude::*};
use async_tungstenite::{tungstenite, WebSocketStream};
use futures::{
    sink::{Sink, SinkExt},
    stream::StreamExt,
    AsyncRead, AsyncWrite,
};
use structopt::StructOpt;
use tungstenite::{Error as WsError, Message as WsMessage};

trait WebSocketConnection:
    Sink<WsMessage, Error = WsError> + Stream<Item = Result<WsMessage, WsError>> + Unpin + Send
{
}
impl<S> WebSocketConnection for WebSocketStream<S> where S: AsyncRead + AsyncWrite + Unpin + Send {}

async fn run(args: Args) -> Result<(), anyhow::Error> {
    let try_socket = TcpListener::bind(&args.server).await;
    let listener = try_socket.expect("Failed to bind");
    println!("Signalling server on: {}", args.server);

    let mut ws_accept_stream = listener.incoming().fuse();

    // Create our application state
    let app = App::new(args).await?;
    // let mut send_gst_msg_rx = send_gst_msg_rx.fuse();

    // let mut connections: Vec<Box<dyn WebSocketConnection>> = Default::default();

    // task::spawn(async {
    //     while let Some(message) = connection.next().await {
    //         let message = message?;

    //         match message {
    //             WsMessage::Close(_) => {
    //                 println!("peer disconnected");
    //                 break;
    //             }
    //             WsMessage::Ping(data) => {
    //                 todo!(); // Some(WsMessage::Pong(data))
    //             }
    //             WsMessage::Pong(_) => {}
    //             WsMessage::Binary(_) => {}
    //             WsMessage::Text(text) => {
    //                 println!("{}", text);
    //                 app.handle_websocket_message(&text)?;
    //             }
    //         };
    //     }
    // });

    // And now let's start our message loop
    loop {
        futures::select! {
            tcp_stream = ws_accept_stream.select_next_some() => {
                let tcp_stream = tcp_stream?;

                let addr = tcp_stream.peer_addr().unwrap();

                println!("Peer address: {}", addr);

                let mut ws_stream = async_tungstenite::accept_async(tcp_stream)
                    .await
                    .unwrap();

                println!("New WebSocket connection: {}", addr);

                let maybe_offer = &*app.offer.lock().unwrap();
                let offer = maybe_offer.as_ref().unwrap().to_string();
                ws_stream.send(WsMessage::Text(
                    serde_json::to_string(&JsonMsg::Sdp(offer))?
                )).await?;

                // connections.push(Box::new(ws_stream));
            },

            // candidate = ice_stream.select_next_some() => {
            //     //
            // }

            // Pass the GStreamer messages to the application control logic
            // gst_msg = send_gst_msg_rx.select_next_some() => {
            //     app.handle_pipeline_message(&gst_msg)?;
            // },


            // Once we're done, break the loop and return
            complete => {
                break
            },
        };
    }

    Ok(())
}

// Check if all GStreamer plugins we require are available
fn check_plugins() -> Result<(), anyhow::Error> {
    let needed = [
        "videotestsrc",
        "audiotestsrc",
        "videoconvert",
        "audioconvert",
        "autodetect",
        "opus",
        "vpx",
        "webrtc",
        "nice",
        "dtls",
        "srtp",
        "rtpmanager",
        "rtp",
        "playback",
        "videoscale",
        "audioresample",
    ];

    let registry = gstreamer::Registry::get();
    let missing = needed
        .iter()
        .filter(|n| registry.find_plugin(n).is_none())
        .cloned()
        .collect::<Vec<_>>();

    if !missing.is_empty() {
        bail!("Missing plugins: {:?}", missing);
    } else {
        Ok(())
    }
}

#[async_std::main]
async fn main() -> Result<(), anyhow::Error> {
    // Initialize GStreamer first
    gstreamer::init()?;
    check_plugins()?;

    let args = Args::from_args();

    // task::spawn(async move {
    //     signalling::run(args.server.parse().unwrap()).await?;
    // });

    // All good, let's run our message loop
    run(args).await?;

    Ok(())
}
