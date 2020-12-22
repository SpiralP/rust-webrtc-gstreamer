// $env:GST_DEBUG = "webrtc*:9"

use crate::{types::JsonMsg, Args};
use anyhow::*;
use async_std::{
    net::{TcpListener, TcpStream},
    prelude::*,
    task,
};
use async_tungstenite::{tungstenite, WebSocketStream};
use futures::{
    channel::{mpsc, oneshot},
    future::FutureExt,
    sink::{Sink, SinkExt},
    stream::StreamExt,
    AsyncRead, AsyncWrite,
};
use gstreamer::{
    gst_element_error, message::MessageView, prelude::*, Caps, Element, ElementFactory,
    PadProbeReturn, PadProbeType, Pipeline, State,
};
use gstreamer_sdp::*;
use gstreamer_webrtc::*;
use std::{
    net::ToSocketAddrs,
    sync::{Arc, Mutex, Weak},
};
use tungstenite::{Error as WsError, Message as WsMessage};

// upgrade weak reference or return
#[macro_export]
macro_rules! upgrade_weak {
    ($x:ident, $r:expr) => {{
        match $x.upgrade() {
            Some(o) => o,
            None => return $r,
        }
    }};
    ($x:ident) => {
        upgrade_weak!($x, ())
    };
}

// Actual application state
pub struct AppInner {
    args: Args,
    pipeline: Pipeline,
    video_tee: Element,
    audio_tee: Element,

    // (webrtcbin, websocket stream)
    clients: Vec<(Element, Box<dyn WebSocketConnection>)>,
}

// Make sure to shut down the pipeline when it goes out of scope
// to release any system resources
impl Drop for AppInner {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(State::Null);
    }
}

// Weak reference to our application state
#[derive(Clone)]
struct AppWeak(Weak<AppInner>);

impl AppWeak {
    // Try upgrading a weak reference to a strong one
    fn upgrade(&self) -> Option<App> {
        self.0.upgrade().map(App)
    }
}

// Strong reference to our application state
#[derive(Clone)]
pub struct App(Arc<AppInner>);

// To be able to access the App's fields directly
impl std::ops::Deref for App {
    type Target = AppInner;

    fn deref(&self) -> &AppInner {
        &self.0
    }
}

fn link(elements: &[&Element], last: Element) -> Result<Element> {
    let mut prev: Option<&Element> = None;

    for &e in elements {
        if let Some(prev) = prev {
            prev.link(e)?;
        }
        prev = Some(e);
    }

    if let Some(prev) = prev {
        prev.link(&last)?;
    }

    Ok(last)
}

impl App {
    // Downgrade the strong reference to a weak reference
    fn downgrade(&self) -> AppWeak {
        AppWeak(Arc::downgrade(&self.0))
    }

    // webrtcbin bundle-policy=max-bundle name=sendrecv stun-server=stun://stun.l.google.com:19302 \

    pub fn new(args: Args) -> Result<Self> {
        let (pipeline, video_tee, audio_tee) = Self::setup(&args)?;

        Ok(App(Arc::new(AppInner {
            args,
            pipeline,
            video_tee,
            audio_tee,
            clients: Default::default(),
        })))
    }

    /// returns (pipeline, video_tee, audio_tee)
    fn setup(args: &Args) -> Result<(Pipeline, Element, Element)> {
        let pipeline = format!(
            concat!(
                "tcpserversrc host={ip} port={port} ! ",
                "tsparse ! tsdemux name=demux ",
                ////////////////
                // video
                "demux. ! queue ! ",
                // decode h264
                "h264parse ! avdec_h264 ! ",
                // encode vp8 (rtp)
                "videoconvert ! videoscale ! videorate ! ",
                "video/x-raw,width=1280,height=720,framerate=30/1 ! ",
                //
                "vp8enc ",
                "cpu-used={cpu_used} ",
                "deadline=1 ",
                "threads={threads} ",
                "static-threshold=0 ",
                "max-intra-bitrate=300 ",
                "lag-in-frames=0 ",
                "min-quantizer=4 ",
                "max-quantizer=48 ",
                "error-resilient=1 ",
                "target-bitrate={video_bitrate} ",
                //
                " ! video/x-vp8 ! ",
                "rtpvp8pay pt=96 ! ",
                "tee name=video-tee ! queue ! fakesink sync=true ",
                ////////////////
                // audio
                "demux. ! queue ! ",
                // decode aac
                "aacparse ! avdec_aac ! ",
                // encode opus (rtp)
                "audioconvert ! audioresample ! ",
                "opusenc ! rtpopuspay pt=97 ! ",
                "tee name=audio-tee ! queue ! fakesink sync=true ",
            ),
            ip = args.stream_server.ip(),
            port = args.stream_server.port(),
            threads = num_cpus::get(),
            cpu_used = args.cpu_used,
            video_bitrate = args.video_bitrate
        );
        let pipeline = gstreamer::parse_launch(&pipeline)?;
        let pipeline = pipeline.downcast::<Pipeline>().expect("not a pipeline");

        let video_tee = pipeline.get_by_name("video-tee").unwrap();
        let audio_tee = pipeline.get_by_name("audio-tee").unwrap();

        Ok((pipeline, video_tee, audio_tee))
    }

    // connect, sdp offer then get answer
    // <- { sdp: "offer....." } // contains 1 candidate
    // -> { sdp: "answer....." }

    pub async fn run(self) -> Result<()> {
        let try_socket = TcpListener::bind(&self.args.signal_server).await;
        let listener = try_socket.expect("Failed to bind");
        println!("Signalling server on: {}", self.args.signal_server);

        let bus = self.pipeline.get_bus().unwrap();
        let mut pipeline_bus_stream = bus.stream().fuse();

        let mut websocket_accept_stream = listener.incoming().fuse();

        self.pipeline.call_async(|pipeline| {
            println!("Setting Pipeline to Playing");
            pipeline
                .set_state(gstreamer::State::Playing)
                .expect("Couldn't set pipeline to Playing");
        });

        loop {
            futures::select! {
                message = pipeline_bus_stream.select_next_some() => {
                    match message.view() {
                        MessageView::Error(err) => bail!(
                            "Error from element {}: {} ({})",
                            err.get_src()
                                .map(|s| String::from(s.get_path_string()))
                                .unwrap_or_else(|| String::from("None")),
                            err.get_error(),
                            err.get_debug().unwrap_or_else(|| String::from("None")),
                        ),

                        MessageView::Warning(warning) => {
                            println!("Warning: \"{}\"", warning.get_debug().unwrap());
                        }

                        _ => {}
                    }
                },

                // handle new websocket connections
                tcp_stream = websocket_accept_stream.select_next_some() => {
                    let tcp_stream = tcp_stream?;

                    let addr = tcp_stream.peer_addr().unwrap();
                    println!("New WebSocket connection: {}", addr);

                    let pipeline = self.pipeline.clone();
                    let video_tee = self.video_tee.clone();
                    let audio_tee = self.audio_tee.clone();

                    task::spawn(async move {
                        Self::on_new_client(pipeline, video_tee, audio_tee, tcp_stream)
                            .await
                            .unwrap();

                        println!("done");
                    });
                },
            };
        }

        println!("select finished!");

        Ok(())
    }

    async fn on_new_client(
        pipeline: Pipeline,
        video_tee: Element,
        audio_tee: Element,
        tcp_stream: TcpStream,
    ) -> Result<()> {
        let mut ws_stream = async_tungstenite::accept_async(tcp_stream).await?.fuse();

        let peer_bin = gstreamer::parse_bin_from_description(
            concat!(
                "queue name=video-queue ! webrtcbin. ",
                "queue name=audio-queue ! webrtcbin. ",
                "webrtcbin name=webrtcbin"
            ),
            false,
        )?;

        let webrtcbin = peer_bin
            .get_by_name("webrtcbin")
            .expect("can't find webrtcbin");

        // Set some properties on webrtcbin
        webrtcbin.set_property_from_str("stun-server", "stun://stun.l.google.com:19302");
        webrtcbin.set_property_from_str("bundle-policy", "max-bundle");
        let ice_agent = webrtcbin
            .get_property("ice-agent")
            .unwrap()
            .get::<gstreamer::Object>()
            .expect("downcast")
            .expect("option");

        // assert_eq!(
        //     ice_agent
        //         .emit("add-local-ip-address", &[&"68.107.26.30"],)
        //         .unwrap()
        //         .unwrap()
        //         .get::<bool>()
        //         .unwrap()
        //         .unwrap(),
        //     true
        // );

        // assert_eq!(
        //     ice_agent
        //         .emit("add-local-ip-address", &[&"10.0.0.2"],)
        //         .unwrap()
        //         .unwrap()
        //         .get::<bool>()
        //         .unwrap()
        //         .unwrap(),
        //     true
        // );

        // assert_eq!(
        //     ice_agent
        //         .emit("add-local-ip-address", &[&"127.0.0.1"],)
        //         .unwrap()
        //         .unwrap()
        //         .get::<bool>()
        //         .unwrap()
        //         .unwrap(),
        //     true
        // );

        webrtcbin.add_property_notify_watch(None, true);

        let bus = webrtcbin.get_bus().unwrap();
        let mut bus_stream = bus.stream().fuse();

        let mut offer_stream = {
            let (sender, receiver) = mpsc::unbounded();

            // Connect to on-negotiation-needed to handle sending an Offer
            webrtcbin.connect("on-negotiation-needed", false, move |values| {
                let webrtcbin = values[0].get::<Element>().unwrap().unwrap();

                println!("on-negotiation-needed");

                let promise = {
                    let webrtcbin = webrtcbin.clone();
                    let sender = sender.clone();

                    gstreamer::Promise::with_change_func(move |reply| {
                        println!("create-offer callback");

                        let structure = reply.unwrap().unwrap();
                        let description = structure
                            .get::<WebRTCSessionDescription>("offer")
                            .expect("Invalid argument")
                            .unwrap();

                        let sdp_text = description.get_sdp().as_text().unwrap();

                        let promise = gstreamer::Promise::with_change_func(move |_| {
                            sender.unbounded_send(JsonMsg::Sdp(sdp_text)).unwrap();
                        });

                        webrtcbin
                            .emit("set-local-description", &[&description, &promise])
                            .unwrap();
                    })
                };

                println!(
                    "create-offer {:#?}",
                    webrtcbin
                        .emit("create-offer", &[&None::<gstreamer::Structure>, &promise])
                        .unwrap()
                );

                None
            })?;

            receiver
        }
        .fuse();

        let mut ice_candidate_stream = {
            let (sender, receiver) = mpsc::unbounded();

            // Connect to on-negotiation-needed to handle sending an Offer
            webrtcbin.connect("on-ice-candidate", false, move |values| {
                let _webrtcbin = values[0].get::<gstreamer::Element>().unwrap().unwrap();
                let sdp_m_line_index = values[1].get::<u32>().unwrap().unwrap();
                let candidate = values[2].get::<String>().unwrap().unwrap();

                // println!("on-ice-candidate: {} {}", sdp_m_line_index, candidate);
                sender
                    .unbounded_send(JsonMsg::Ice {
                        sdp_m_line_index,
                        candidate,
                    })
                    .unwrap();

                None
            })?;

            receiver
        }
        .fuse();

        // Add ghost pads for connecting to the input
        let audio_queue = peer_bin
            .get_by_name("audio-queue")
            .expect("can't find audio-queue");
        let audio_sink_pad = gstreamer::GhostPad::with_target(
            Some("audio_sink"),
            &audio_queue.get_static_pad("sink").unwrap(),
        )
        .unwrap();
        peer_bin.add_pad(&audio_sink_pad).unwrap();

        let video_queue = peer_bin
            .get_by_name("video-queue")
            .expect("can't find video-queue");
        let video_sink_pad = gstreamer::GhostPad::with_target(
            Some("video_sink"),
            &video_queue.get_static_pad("sink").unwrap(),
        )
        .unwrap();
        peer_bin.add_pad(&video_sink_pad).unwrap();

        pipeline.add(&peer_bin).unwrap();

        // {
        //     let transceiver = webrtcbin
        //         .emit("get-transceiver", &[&0i32])
        //         .unwrap()
        //         .unwrap();
        //     let transceiver = transceiver.get::<WebRTCRTPTransceiver>().unwrap().unwrap();
        //     transceiver.set_property_direction(WebRTCRTPTransceiverDirection::Sendonly);
        // }

        ////////////////////////////////////////////////////////////////

        // Add pad probes to both tees for blocking them and
        // then unblock them once we reached the Playing state.
        //
        // Then link them and unblock, in case they got blocked
        // in the meantime.
        //
        // Otherwise it might happen that data is received before
        // the elements are ready and then an error happens.
        let audio_src_pad = audio_tee.get_request_pad("src_%u").unwrap();
        let audio_block = audio_src_pad
            .add_probe(gstreamer::PadProbeType::BLOCK_DOWNSTREAM, |_pad, _info| {
                gstreamer::PadProbeReturn::Ok
            })
            .unwrap();
        audio_src_pad.link(&audio_sink_pad)?;

        let video_src_pad = video_tee.get_request_pad("src_%u").unwrap();
        let video_block = video_src_pad
            .add_probe(gstreamer::PadProbeType::BLOCK_DOWNSTREAM, |_pad, _info| {
                gstreamer::PadProbeReturn::Ok
            })
            .unwrap();
        video_src_pad.link(&video_sink_pad)?;

        peer_bin.connect_pad_added(move |_bin, pad| {
            println!("!!!!!!!!!!!! {}", pad.get_name());
        });

        // Asynchronously set the peer bin to Playing
        peer_bin.call_async(move |bin| {
            // If this fails, post an error on the bus so we exit
            if bin.sync_state_with_parent().is_err() {
                gst_element_error!(
                    bin,
                    gstreamer::LibraryError::Failed,
                    ("Failed to set peer bin to Playing")
                );
            }

            // And now unblock
            println!("unblocking");
            audio_src_pad.remove_probe(audio_block);
            video_src_pad.remove_probe(video_block);
        });

        // Asynchronously set the pipeline to Playing
        pipeline.call_async(|pipeline| {
            // If this fails, post an error on the bus so we exit
            if pipeline.set_state(gstreamer::State::Playing).is_err() {
                gst_element_error!(
                    pipeline,
                    gstreamer::LibraryError::Failed,
                    ("Failed to set pipeline to Playing")
                );
            }
        });

        if let Some(message) = offer_stream.next().await {
            println!("set-local-description");
        }

        println!("loop");
        loop {
            futures::select! {
                message = offer_stream.select_next_some() => {
                    todo!("more than 1 offer/negotiation?");
                },

                json_msg = ice_candidate_stream.select_next_some() => {
                    // println!("send candidate: {:?}", json_msg);

                    // ws_stream
                    //     .send(WsMessage::Text(serde_json::to_string(&json_msg)?))
                    //     .await?;
                },

                message = bus_stream.select_next_some() => {
                    match message.view() {
                        MessageView::PropertyNotify(notify) => {
                            println!("PropertyNotify");
                            let structure = notify.get_structure().unwrap();
                            let property_name = structure.get::<String>("property-name").unwrap().unwrap();
                            // GstMessagePropertyNotify, property-name=(string)ice-gathering-state, property-value=(GstWebRTCICEGatheringState)complete;
                            if property_name == "ice-gathering-state" {
                                let property_value = structure.get::<WebRTCICEGatheringState>("property-value").unwrap().unwrap();
                                println!("!!!!! {:#?}", structure);
                                if property_value == WebRTCICEGatheringState::Complete {
                                    println!("finished gathering ice candidates!");

                                    let description = webrtcbin
                                        .get_property("pending-local-description")
                                        .unwrap()
                                        .get::<WebRTCSessionDescription>()
                                        .unwrap().unwrap();

                                    let offer = description.get_sdp().as_text().unwrap();

                                    println!("send offer:");
                                    println!("{}", offer);

                                    ws_stream
                                        .send(WsMessage::Text(serde_json::to_string(&JsonMsg::Sdp(
                                            offer,
                                        ))?))
                                        .await?;
                                }
                            } else {
                                println!("{:#?}", structure);
                            }
                        }

                        _ => {}
                    }
                },


                result = ws_stream.select_next_some() => {
                    let message = result?;

                    match message {
                        WsMessage::Close(_) => {
                            println!("peer disconnected");

                            // Block the tees shortly for removal
                            let audio_tee_sinkpad = audio_tee.get_static_pad("sink").unwrap();
                            let audio_block = audio_tee_sinkpad
                                .add_probe(gstreamer::PadProbeType::BLOCK_DOWNSTREAM, |_pad, _info| {
                                    gstreamer::PadProbeReturn::Ok
                                })
                                .unwrap();

                            let video_tee_sinkpad = video_tee.get_static_pad("sink").unwrap();
                            let video_block = video_tee_sinkpad
                                .add_probe(gstreamer::PadProbeType::BLOCK_DOWNSTREAM, |_pad, _info| {
                                    gstreamer::PadProbeReturn::Ok
                                })
                                .unwrap();

                            // Release the tee pads and unblock
                            let audio_sinkpad = peer_bin.get_static_pad("audio_sink").unwrap();
                            let video_sinkpad = peer_bin.get_static_pad("video_sink").unwrap();

                            if let Some(audio_tee_srcpad) = audio_sinkpad.get_peer() {
                                let _ = audio_tee_srcpad.unlink(&audio_sinkpad);
                                audio_tee.release_request_pad(&audio_tee_srcpad);
                            }
                            audio_tee_sinkpad.remove_probe(audio_block);

                            if let Some(video_tee_srcpad) = video_sinkpad.get_peer() {
                                let _ = video_tee_srcpad.unlink(&video_sinkpad);
                                video_tee.release_request_pad(&video_tee_srcpad);
                            }
                            video_tee_sinkpad.remove_probe(video_block);

                            // Then remove the peer bin gracefully from the pipeline
                            let _ = pipeline.remove(&peer_bin);
                            let _ = peer_bin.set_state(gstreamer::State::Null);



                            println!("Removed");
                            break;
                        }
                        WsMessage::Ping(data) => {
                            ws_stream.send(WsMessage::Pong(data)).await?;
                        }
                        WsMessage::Pong(_) => {}
                        WsMessage::Binary(_) => {}
                        WsMessage::Text(text) => {
                            let message = serde_json::from_str::<JsonMsg>(&text)?;

                            match message {
                                JsonMsg::Sdp(sdp_answer) => {
                                    let ret = SDPMessage::parse_buffer(sdp_answer.as_bytes())
                                        .map_err(|_| anyhow!("Failed to parse SDP answer"))?;
                                    let description = WebRTCSessionDescription::new(
                                        WebRTCSDPType::Answer,
                                        ret,
                                    );

                                    let (sender, receiver) = oneshot::channel();
                                    let promise = gstreamer::Promise::with_change_func(move |reply| {
                                        sender.send(()).unwrap();
                                    });

                                    println!("set-remote-description");
                                    webrtcbin
                                        .emit(
                                            "set-remote-description",
                                            &[&description, &promise],
                                        )
                                        .unwrap();

                                    receiver.await.unwrap();
                                }

                                JsonMsg::Ice { sdp_m_line_index, candidate } => {
                                    if candidate != "" {
                                        println!("add-ice-candidate");
                                        println!("{}", candidate);

                                        webrtcbin
                                        .emit(
                                            "add-ice-candidate",
                                            &[&sdp_m_line_index, &candidate],
                                        )
                                        .unwrap();
                                    }
                                }
                            }
                        }
                    };
                },
            };
        }

        Ok(())
    }
}

trait WebSocketConnection:
    Sink<WsMessage, Error = WsError> + Stream<Item = Result<WsMessage, WsError>> + Unpin + Send
{
}
impl<S> WebSocketConnection for WebSocketStream<S> where S: AsyncRead + AsyncWrite + Unpin + Send {}

trait PipelineMake {
    fn make(&self, kind: &str) -> Result<Element>;
}
impl PipelineMake for Pipeline {
    fn make(&self, kind: &str) -> Result<Element> {
        let element = ElementFactory::make(kind, None)?;
        self.add(&element)?;
        Ok(element)
    }
}
