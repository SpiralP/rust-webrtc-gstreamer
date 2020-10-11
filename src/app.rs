use crate::{types::JsonMsg, Args};
use anyhow::*;
use async_std::{
    net::{TcpListener, TcpStream},
    prelude::*,
    task,
};
use async_tungstenite::{tungstenite, WebSocketStream};
use futures::{
    channel::mpsc,
    future::FutureExt,
    sink::{Sink, SinkExt},
    stream::StreamExt,
    AsyncRead, AsyncWrite,
};
use gstreamer::{
    gst_element_error, message::MessageView, prelude::*, Caps, Element, ElementFactory,
    PadProbeReturn, PadProbeType, Pipeline, State,
};
use gstreamer_webrtc::*;
use std::sync::{Arc, Mutex, Weak};
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
        let (pipeline, video_tee, audio_tee) = Self::setup()?;

        Ok(App(Arc::new(AppInner {
            args,
            pipeline,
            video_tee,
            audio_tee,
            clients: Default::default(),
        })))
    }

    /// returns (video_tee, audio_tee)
    fn setup() -> Result<(Pipeline, Element, Element)> {
        // videotestsrc -> videoconvert -> vp8enc -> rtpvp8pay -> tee -> { queue -> filesink }

        let pipeline = gstreamer::parse_launch(
            "videotestsrc is-live=true ! videoconvert ! vp8enc deadline=1 ! rtpvp8pay pt=96 ! tee \
             name=video-tee ! queue ! fakesink sync=true",
        )?;

        let pipeline = pipeline.downcast::<Pipeline>().expect("not a pipeline");

        let video_tee = pipeline.get_by_name("video-tee").unwrap();

        // {
        //     let filesink = link(
        //         &[&video_tee, &pipeline.make("queue")?],
        //         pipeline.make("filesink")?,
        //     )?;
        //     filesink
        //         .set_property("location", &"target/ag.webm")
        //         .unwrap();
        // }
        // {
        //     let filesink = link(
        //         &[&video_tee, &pipeline.make("queue")?],
        //         pipeline.make("filesink")?,
        //     )?;
        //     filesink
        //         .set_property("location", &"target/ag2.webm")
        //         .unwrap();
        // }

        // videotestsrc is-live=true pattern=ball
        // videoconvert
        // queue
        // vp8enc deadline=1
        // rtpvp8pay
        // queue
        // application/x-rtp,media=video,encoding-name=VP8,payload=96
        //
        // sendrecv.
        // audiotestsrc is-live=true wave=red-noise
        // audioconvert
        // audioresample
        // queue
        // opusenc
        // rtpopuspay
        // queue
        // application/x-rtp,media=audio,encoding-name=OPUS,payload=97
        //
        // sendrecv.
        //

        // TODO
        let audio_tee = ElementFactory::make("queue", None)?;

        Ok((pipeline, video_tee, audio_tee))
    }

    // // Handle incoming ICE candidates from the peer by passing them to webrtcbin
    // fn handle_ice(&self, sdp_m_line_index: u32, candidate: &str) -> Result<()> {
    //     println!("recv {}", candidate);
    //     self.webrtcbin
    //         .emit("add-ice-candidate", &[&sdp_m_line_index, &candidate])
    //         .unwrap();

    //     Ok(())
    // }

    // // Asynchronously send ICE candidates to the peer via the WebSocket connection as a JSON
    // // message
    // fn on_ice_candidate(&self, mlineindex: u32, candidate: String) -> Result<()> {
    //     println!("{} {}", mlineindex, candidate);

    //     let candidates = &mut *self.candidates.lock().unwrap();
    //     candidates.push(WsMessage::Text(
    //         serde_json::to_string(&JsonMsg::Ice {
    //             candidate,
    //             sdp_m_line_index: mlineindex,
    //         })
    //         .unwrap(),
    //     ));

    //     Ok(())
    // }

    // connect, sdp offer then get answer
    // <- { sdp: "offer....." } // contains 1 candidate
    // -> { sdp: "answer....." }

    pub async fn run(self) -> Result<()> {
        let try_socket = TcpListener::bind(&self.args.server).await;
        let listener = try_socket.expect("Failed to bind");
        println!("Signalling server on: {}", self.args.server);

        let bus = self.pipeline.get_bus().unwrap();
        let mut message_stream = bus.stream();
        let mut message_handle = async move {
            while let Some(message) = message_stream.next().await {
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
            }

            Ok(())
        }
        .boxed_local()
        .fuse();

        let pipeline = self.pipeline.clone();

        // handle new websocket connections
        let mut ws_accept_stream = listener.incoming().fuse();
        let app = self;
        let mut websocket_accept_handle = async move {
            let pipeline = app.pipeline.clone();
            let video_tee = app.video_tee.clone();

            while let Some(tcp_stream) = ws_accept_stream.next().await {
                let tcp_stream = tcp_stream?;

                let addr = tcp_stream.peer_addr().unwrap();
                println!("New WebSocket connection: {}", addr);

                let pipeline = pipeline.clone();
                let video_tee = video_tee.clone();

                task::spawn(async move {
                    Self::on_new_client(pipeline, video_tee, tcp_stream)
                        .await
                        .unwrap();

                    println!("done");
                });
            }

            Ok::<_, Error>(())
        }
        .boxed_local()
        .fuse();

        pipeline.call_async(|pipeline| {
            println!("Setting Pipeline to Playing");
            pipeline
                .set_state(gstreamer::State::Playing)
                .expect("Couldn't set pipeline to Playing");
        });

        futures::select! {
            result = message_handle => {
                println!("message_handle: {:#?}", result);
                result?;
            },

            result = websocket_accept_handle => {
                println!("websocket_accept_handle: {:#?}", result);
                result?;
            },
        };

        println!("select finished!");

        Ok(())
    }

    async fn on_new_client(
        pipeline: Pipeline,
        video_tee: Element,
        tcp_stream: TcpStream,
    ) -> Result<()> {
        let mut ws_stream = async_tungstenite::accept_async(tcp_stream).await?.fuse();

        // let webrtcbin = pipeline.make("webrtcbin")?;
        // webrtcbin
        //     .set_property("bundle-policy", &WebRTCBundlePolicy::MaxBundle)
        //     .unwrap();
        // webrtcbin
        //     .set_property("stun-server", &"stun://stun.l.google.com:19302")
        //     .unwrap();

        let peer_bin = gstreamer::parse_bin_from_description(
            "queue name=video-queue ! webrtcbin name=webrtcbin",
            false,
        )?;

        // Get access to the webrtcbin by name
        let webrtcbin = peer_bin
            .get_by_name("webrtcbin")
            .expect("can't find webrtcbin");

        // Set some properties on webrtcbin
        webrtcbin.set_property_from_str("stun-server", "stun://stun.l.google.com:19302");
        webrtcbin.set_property_from_str("bundle-policy", "max-bundle");

        let mut offer_stream = {
            let (sender, receiver) = mpsc::unbounded();

            // Connect to on-negotiation-needed to handle sending an Offer
            webrtcbin.connect("on-negotiation-needed", false, move |values| {
                println!("on-negotiation-needed");

                let webrtcbin = values[0].get::<gstreamer::Element>().unwrap().unwrap();

                let promise = {
                    let webrtcbin = webrtcbin.clone();
                    let sender = sender.clone();

                    gstreamer::Promise::with_change_func(move |reply| {
                        println!("create-offer callback");

                        let result = match reply {
                            Ok(None) => Err(anyhow!("Offer creation future got no reponse")),

                            Err(err) => Err(anyhow!(
                                "Offer creation future got error reponse: {:?}",
                                err
                            )),

                            Ok(Some(reply)) => {
                                let offer = reply
                                    .get_value("offer")
                                    .unwrap()
                                    .get::<gstreamer_webrtc::WebRTCSessionDescription>()
                                    .expect("Invalid argument")
                                    .unwrap();

                                webrtcbin
                                    .emit(
                                        "set-local-description",
                                        &[&offer, &None::<gstreamer::Promise>],
                                    )
                                    .unwrap();

                                let sdp_text = offer.get_sdp().as_text().unwrap();
                                Ok(sdp_text)
                            }
                        };

                        sender.unbounded_send(result).unwrap();

                        // if let Err(err) = result {
                        //     gst_element_error!(
                        //         pipeline,
                        //         gstreamer::LibraryError::Failed,
                        //         ("Failed to send SDP offer: {:?}", err)
                        //     );
                        // }
                    })
                };

                webrtcbin
                    .emit("create-offer", &[&None::<gstreamer::Structure>, &promise])
                    .unwrap();

                None
            })?;

            receiver
        }
        .fuse();

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

        let video_src_pad = video_tee.get_request_pad("src_%u").unwrap();
        let video_block = video_src_pad
            .add_probe(gstreamer::PadProbeType::BLOCK_DOWNSTREAM, |_pad, _info| {
                gstreamer::PadProbeReturn::Ok
            })
            .unwrap();
        video_src_pad.link(&video_sink_pad).unwrap();

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
            video_src_pad.remove_probe(video_block);
        });

        loop {
            futures::select! {
                result = offer_stream.select_next_some() => {
                    let offer = result.unwrap();

                    println!("sending offer:");
                    println!("{}", offer);

                    ws_stream
                        .send(WsMessage::Text(serde_json::to_string(&JsonMsg::Sdp(
                            offer,
                        ))?))
                        .await?;
                },


                result = ws_stream.select_next_some() => {
                    let message = result?;

                    match message {
                        WsMessage::Close(_) => {
                            println!("peer disconnected");

                            let video_tee_sinkpad = video_tee.get_static_pad("sink").unwrap();
                            let video_block = video_tee_sinkpad
                                .add_probe(gstreamer::PadProbeType::BLOCK_DOWNSTREAM, |_pad, _info| {
                                    gstreamer::PadProbeReturn::Ok
                                })
                                .unwrap();

                            // Release the tee pads and unblock
                            let video_sinkpad = peer_bin.get_static_pad("video_sink").unwrap();

                            if let Some(video_tee_srcpad) = video_sinkpad.get_peer() {
                                let _ = video_tee_srcpad.unlink(&video_sinkpad);
                                video_tee.release_request_pad(&video_tee_srcpad);
                            }
                            video_tee_sinkpad.remove_probe(video_block);

                            // Then remove the peer bin gracefully from the pipeline
                            pipeline.remove(&peer_bin).unwrap();
                            peer_bin.set_state(gstreamer::State::Null).unwrap();

                            println!("Removed");


                            // webrtcbin.set_state(gstreamer::State::Null)?;
                            // queue.set_state(gstreamer::State::Null)?;

                            // pipeline.remove(&webrtcbin)?;
                            // pipeline.remove(&queue)?;
                            break;
                        }
                        WsMessage::Ping(data) => {
                            ws_stream.send(WsMessage::Pong(data)).await?;
                        }
                        WsMessage::Pong(_) => {}
                        WsMessage::Binary(_) => {}
                        WsMessage::Text(text) => {
                            println!("{}", text);


                            // println!("Received answer:");
                            // println!("{}", sdp);

                            // let ret = gstreamer_sdp::SDPMessage::parse_buffer(sdp.as_bytes())
                            //     .map_err(|_| anyhow!("Failed to parse SDP answer"))?;
                            // let answer = gstreamer_webrtc::WebRTCSessionDescription::new(
                            //     gstreamer_webrtc::WebRTCSDPType::Answer,
                            //     ret,
                            // );

                            // self.webrtcbin
                            //     .emit(
                            //         "set-remote-description",
                            //         &[&answer, &None::<gstreamer::Promise>],
                            //     )
                            //     .unwrap();
                        }
                    };
                },
            };
        }

        // // Whenever there is a new ICE candidate, send it to the peer
        // let app_clone = app.downgrade();
        // app.webrtcbin
        //     .connect("on-ice-candidate", false, move |values| {
        //         let _webrtc = values[0]
        //             .get::<gstreamer::Element>()
        //             .expect("Invalid argument");
        //         let mlineindex = values[1].get_some::<u32>().expect("Invalid argument");
        //         let candidate = values[2]
        //             .get::<String>()
        //             .expect("Invalid argument")
        //             .unwrap();

        //         let app = upgrade_weak!(app_clone, None);

        //         if let Err(err) = app.on_ice_candidate(mlineindex, candidate) {
        //             gst_element_error!(
        //                 app.pipeline,
        //                 gstreamer::LibraryError::Failed,
        //                 ("Failed to send ICE candidate: {:?}", err)
        //             );
        //         }

        //         None
        //     })
        //     .unwrap();

        // std::thread::sleep_ms(3000);

        // println!("adding more!");
        // {
        // }

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
