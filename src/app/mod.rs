mod stats;
mod types;

pub use self::types::Args;
use self::{stats::Stats, types::JsonMsg};
use crate::{concat_spaces, upgrade_weak};
use anyhow::{anyhow, bail, Result};
use futures::{
    channel::{mpsc, oneshot},
    future::FutureExt,
    lock::Mutex,
    sink::SinkExt,
    stream::StreamExt,
};
use gstreamer::{
    element_error, message::MessageView, prelude::*, Element, ElementFactory, Pipeline,
};
use gstreamer_sdp::SDPMessage;
use gstreamer_webrtc::*;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs},
    ops,
    os::raw::c_int,
    sync::{Arc, Weak},
    time::Duration,
};
use tracing::*;
use warp::{path::FullPath, ws, Filter};

include!(concat!(env!("OUT_DIR"), "/nodejs_bundle.rs"));

const STUN_ADDRESS: &str = "stun.l.google.com:19302";

// Strong reference to our application state
#[derive(Debug, Clone)]
pub struct App(Arc<AppInner>);

// Weak reference to our application state
#[derive(Debug, Clone)]
struct AppWeak(Weak<AppInner>);

impl AppWeak {
    // Try upgrading a weak reference to a strong one
    fn upgrade(&self) -> Option<App> {
        self.0.upgrade().map(App)
    }
}

// Actual application state
#[derive(Debug)]
pub struct AppInner {
    args: Args,
    pipeline: Pipeline,
    video_tee: Element,
    audio_tee: Element,
    stun_address: String,
    stats: Arc<Mutex<Stats>>,
}

// To be able to access the App's fields directly
impl ops::Deref for App {
    type Target = AppInner;

    fn deref(&self) -> &AppInner {
        &self.0
    }
}

impl App {
    // Downgrade the strong reference to a weak reference
    fn downgrade(&self) -> AppWeak {
        AppWeak(Arc::downgrade(&self.0))
    }

    #[tracing::instrument(skip(args))]
    pub fn new(args: Args) -> Result<(Self, Arc<Mutex<Stats>>)> {
        let (pipeline, video_tee, audio_tee) = Self::setup(&args)?;

        // libnice doesn't like ipv6 stun addresses, so resolve an ipv4 one
        let stun_address = STUN_ADDRESS
            .to_socket_addrs()?
            .find(|addr| addr.is_ipv4())
            .map(|addr| format!("{:?}", addr))
            .unwrap_or_else(|| STUN_ADDRESS.to_string());
        info!("using STUN {}", stun_address);

        let stats = Arc::new(Mutex::new(Stats::new()));
        Ok((
            App(Arc::new(AppInner {
                args,
                pipeline,
                video_tee,
                audio_tee,
                stun_address,
                stats: stats.clone(),
            })),
            stats,
        ))
    }

    /// returns (pipeline, video_tee, audio_tee)
    #[tracing::instrument]
    fn setup(args: &Args) -> Result<(Pipeline, Element, Element)> {
        let (input_component, tsparse_set_timestamps) = if args.udp {
            (
                format!(
                    "udpsrc address={ip} port={port} do-timestamp=false",
                    ip = args.stream_server.ip(),
                    port = args.stream_server.port(),
                ),
                "true",
            )
        } else {
            (
                format!(
                    "tcpserversrc host={ip} port={port} do-timestamp=false",
                    ip = args.stream_server.ip(),
                    port = args.stream_server.port(),
                ),
                "false",
            )
        };
        let pipeline = format!(
            concat_spaces!(
                "{input} ! queue",
                // set-timestamps=true makes it jittery on tcp
                "  ! tsparse set-timestamps={tsparse_set_timestamps}",
                "  ! tsdemux name=demux latency=700",
                ////////////////
                // video
                "demux. ! queue",
                // "videotestsrc pattern=snow is-live=true do-timestamp=true ! queue",
                // decode h264
                "  ! decodebin ! queue",
                // encode vp9 (rtp)
                "  ! videoconvert ! videoscale ! videorate ! queue",
                // "  ! video/x-raw,width=1280,height=720,framerate=30/1",
                "  ! vp9enc",
                "    end-usage=cbr",
                "    cpu-used={cpu_used}",
                "    deadline=1",
                "    threads={threads}",
                "    static-threshold=0",
                "    max-intra-bitrate=300",
                "    lag-in-frames=0",
                "    min-quantizer=4",
                "    max-quantizer=48",
                "    error-resilient=1",
                "    target-bitrate={video_bitrate}",
                "  ! video/x-vp9 ! queue",
                "  ! rtpvp9pay pt=96 ! queue",
                "  ! tee name=video-tee ! fakesink",
                // "  ! rtpvp9depay ! vp9dec ! videoconvert ! autovideosink",
                ////////////////
                // audio
                "demux. ! queue",
                // "audiotestsrc is-live=true ! queue",
                // decode aac
                "  ! decodebin ! queue",
                // encode opus (rtp)
                "  ! audioconvert ! audioresample ! queue",
                "  ! audio/x-raw,channels=2",
                "  ! opusenc",
                "    bitrate={audio_bitrate}",
                "  ! audio/x-opus ! queue",
                "  ! rtpopuspay pt=97 ! queue",
                "  ! tee name=audio-tee ! fakesink",
            ),
            input = input_component,
            tsparse_set_timestamps = tsparse_set_timestamps,
            threads = num_cpus::get(),
            cpu_used = args.cpu_used,
            video_bitrate = args.video_bitrate,
            audio_bitrate = args.audio_bitrate,
        );
        let pipeline = gstreamer::parse_launch(&pipeline)?;
        let pipeline = pipeline.downcast::<Pipeline>().expect("not a pipeline");

        let video_tee = pipeline.by_name("video-tee").unwrap();
        let audio_tee = pipeline.by_name("audio-tee").unwrap();

        Ok((pipeline, video_tee, audio_tee))
    }

    // connect, sdp offer then get answer
    // <- { sdp: "offer....." } // contains 1 candidate
    // -> { sdp: "answer....." }

    #[tracing::instrument(skip(self))]
    pub async fn run(self) -> Result<()> {
        let mut pipeline_bus_stream = self.pipeline.bus().unwrap().stream().fuse();
        {
            debug!("Setting Pipeline to Playing");
            self.pipeline
                .set_state(gstreamer::State::Playing)
                .expect("Couldn't set pipeline to Playing");

            let mut stats = self.stats.lock().await;
            stats.set_state(stats::State::WaitingForStream);
        }

        let (mut warp_future, warp_shutdown_signal) = {
            info!(
                "starting http/websocket server on http://{}/",
                &self.args.signal_server
            );

            let weak_app = self.downgrade();
            let routes = warp::path("ws")
                .and(warp::addr::remote().and(warp::ws()).map(
                    move |addr: Option<SocketAddr>, ws: warp::ws::Ws| {
                        let weak_app = weak_app.clone();

                        ws.on_upgrade(move |websocket| async move {
                            let addr = addr.unwrap_or_else(|| {
                                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0)
                            });

                            tokio::spawn(async move {
                                let app = upgrade_weak!(weak_app);

                                {
                                    let mut stats = app.stats.lock().await;
                                    stats.on_client_connected(addr);
                                }
                                app.handle_client_loop(websocket, addr).await.unwrap();
                                {
                                    let mut stats = app.stats.lock().await;
                                    stats.on_client_disconnected(addr);
                                }
                            });
                        })
                    },
                ))
                .or(warp::path::full().map(|path: FullPath| {
                    // debug!("http {}", path.as_str());
                    NODEJS_BUNDLE.as_warp_reply(path)
                }));

            let (warp_shutdown_signal, warp_shutdown_signal_rx) = oneshot::channel();

            let (_addr, warp_server) =
                warp::serve(routes).bind_with_graceful_shutdown(self.args.signal_server, async {
                    let _ = warp_shutdown_signal_rx.await;
                });

            (warp_server.boxed().fuse(), warp_shutdown_signal)
        };

        loop {
            futures::select! {
                message = pipeline_bus_stream.select_next_some() => {
                    if let MessageView::PropertyNotify(notify) = message.view() {
                        let structure = notify.structure().unwrap();
                        debug!("PropertyNotify {:#?}", structure);
                    }

                    if self.handle_pipeline_message(message).await?  {
                        let _ = warp_shutdown_signal.send(());
                        break;
                    }
                },

                _ = warp_future => {
                    break;
                },
            };
        }

        self.pipeline
            .set_state(gstreamer::State::Null)
            .expect("Unable to set the pipeline to the `Null` state");

        Ok(())
    }

    #[tracing::instrument(skip(self, websocket))]
    async fn handle_client_loop(
        &self,
        websocket: warp::ws::WebSocket,
        addr: SocketAddr,
    ) -> Result<()> {
        debug!("new client");

        let (mut ws_sender, ws_receiver) = websocket.split();
        let mut ws_receiver = ws_receiver.fuse();

        let peer_bin = gstreamer::parse_bin_from_description(
            concat_spaces!(
                "queue name=video-queue leaky=downstream",
                "  ! webrtcbin.",
                // "  ! rtpvp9depay ! vp9dec ! videoconvert ! autovideosink",
                "queue name=audio-queue leaky=downstream",
                "  ! webrtcbin.",
                // "  ! fakesink",
                // "  ! rtpopusdepay ! opusdec ! audioconvert ! audioresample ! autoaudiosink",
                "webrtcbin name=webrtcbin latency=100",
            ),
            false,
        )?;

        let webrtcbin = peer_bin.by_name("webrtcbin").expect("can't find webrtcbin");
        webrtcbin.add_property_notify_watch(None, true);

        webrtcbin.set_property_from_str("bundle-policy", "max-bundle");
        webrtcbin.set_property_from_str("stun-server", &format!("stun://{}", self.stun_address));

        let mut offer_stream = {
            let (sender, receiver) = mpsc::unbounded();

            // on-negotiation-needed -> create-offer -> set-local-description
            webrtcbin.connect("on-negotiation-needed", false, move |values| {
                let webrtcbin = values[0].get::<Element>().unwrap();

                // debug!("on-negotiation-needed");

                let promise = {
                    let webrtcbin = webrtcbin.clone();
                    let sender = sender.clone();

                    gstreamer::Promise::with_change_func(move |reply| {
                        // debug!("create-offer callback");

                        let structure = reply.unwrap().unwrap();
                        let description = structure
                            .get::<WebRTCSessionDescription>("offer")
                            .expect("Invalid argument");

                        let sdp_text = description.sdp().as_text().unwrap();

                        let promise = gstreamer::Promise::with_change_func(move |_| {
                            sender.unbounded_send(JsonMsg::Sdp(sdp_text)).unwrap();
                        });

                        webrtcbin
                            .emit_by_name::<()>("set-local-description", &[&description, &promise]);
                    })
                };

                webrtcbin
                    .emit_by_name::<()>("create-offer", &[&None::<gstreamer::Structure>, &promise]);

                None
            });

            receiver.fuse()
        };

        let mut ice_candidate_stream = {
            let (sender, receiver) = mpsc::unbounded();

            webrtcbin.connect("on-ice-candidate", false, move |values| {
                let span = debug_span!("on-ice-candidate");
                let _enter = span.enter();

                let _webrtcbin = values[0].get::<gstreamer::Element>().unwrap();
                let sdp_m_line_index = values[1].get::<u32>().unwrap();
                let candidate = values[2].get::<String>().unwrap();

                // if candidate.contains("srflx") {
                //     debug!("{} {}", sdp_m_line_index, candidate);
                // }
                sender
                    .unbounded_send(JsonMsg::Ice {
                        sdp_m_line_index,
                        candidate,
                    })
                    .unwrap();

                None
            });

            receiver.fuse()
        };

        let mut total_bytes_sent_stream = {
            let (sender, receiver) = mpsc::unbounded();

            let webrtcbin = webrtcbin.clone();
            tokio::spawn(async move {
                while !sender.is_closed() {
                    let sender = sender.clone();
                    let promise = gstreamer::Promise::with_change_func(move |structure| {
                        let structure = structure.unwrap().unwrap();
                        // println!("{:#?}", structure);

                        let mut total_bytes_sent: u64 = 0;

                        // rtp-outbound-stream-stats_
                        //   bytes-sent
                        for field in structure.fields() {
                            if field.starts_with("rtp-outbound-stream-stats_") {
                                let sub_structure =
                                    structure.get::<gstreamer::Structure>(field).unwrap();

                                total_bytes_sent += sub_structure.get::<u64>("bytes-sent").unwrap();
                            }
                        }

                        if total_bytes_sent != 0 {
                            sender.unbounded_send(total_bytes_sent).unwrap();
                        }
                    });

                    webrtcbin.emit_by_name::<()>("get-stats", &[&None::<gstreamer::Pad>, &promise]);

                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            });

            receiver.fuse()
        };

        // Add ghost pads for connecting to the input
        let audio_queue = peer_bin
            .by_name("audio-queue")
            .expect("can't find audio-queue");
        let audio_sink_pad = gstreamer::GhostPad::with_target(
            Some("audio_sink"),
            &audio_queue.static_pad("sink").unwrap(),
        )
        .unwrap();
        peer_bin.add_pad(&audio_sink_pad).unwrap();

        let video_queue = peer_bin
            .by_name("video-queue")
            .expect("can't find video-queue");
        let video_sink_pad = gstreamer::GhostPad::with_target(
            Some("video_sink"),
            &video_queue.static_pad("sink").unwrap(),
        )
        .unwrap();
        peer_bin.add_pad(&video_sink_pad).unwrap();

        self.pipeline.add(&peer_bin).unwrap();

        for i in 0..2 {
            let transceiver =
                webrtcbin.emit_by_name::<WebRTCRTPTransceiver>("get-transceiver", &[&(i as c_int)]);
            transceiver.set_property("direction", WebRTCRTPTransceiverDirection::Sendonly);
        }

        ////////////////////////////////////////////////////////////////

        // Add pad probes to both tees for blocking them and
        // then unblock them once we reached the Playing state.
        //
        // Then link them and unblock, in case they got blocked
        // in the meantime.
        //
        // Otherwise it might happen that data is received before
        // the elements are ready and then an error happens.
        let audio_src_pad = self.audio_tee.request_pad_simple("src_%u").unwrap();
        let audio_block = audio_src_pad
            .add_probe(gstreamer::PadProbeType::BLOCK_DOWNSTREAM, |_pad, _info| {
                gstreamer::PadProbeReturn::Ok
            })
            .unwrap();
        audio_src_pad.link(&audio_sink_pad)?;

        let video_src_pad = self.video_tee.request_pad_simple("src_%u").unwrap();
        let video_block = video_src_pad
            .add_probe(gstreamer::PadProbeType::BLOCK_DOWNSTREAM, |_pad, _info| {
                gstreamer::PadProbeReturn::Ok
            })
            .unwrap();
        video_src_pad.link(&video_sink_pad)?;

        // If this fails, post an error on the bus so we exit
        if peer_bin.sync_state_with_parent().is_err() {
            element_error!(
                peer_bin,
                gstreamer::LibraryError::Failed,
                ("Failed to set peer bin to Playing")
            );
        }

        // And now unblock
        audio_src_pad.remove_probe(audio_block);
        video_src_pad.remove_probe(video_block);

        let mut webrtcbin_bus_stream = webrtcbin.bus().unwrap().stream().fuse();
        if self.pipeline.set_state(gstreamer::State::Playing).is_err() {
            element_error!(
                self.pipeline,
                gstreamer::LibraryError::Failed,
                ("Failed to set pipeline to Playing")
            );
        }

        // wait for offer to generate (local-description is set)
        loop {
            futures::select! {
                offer = offer_stream.select_next_some() => {
                    // debug!("set-local-description");

                    ws_sender
                        .send(ws::Message::text(serde_json::to_string(&offer)?))
                        .await?;

                    break;
                },

                result = ws_receiver.select_next_some() => {
                    let message = result?;

                    if message.is_close() {
                        // debug!("websocket closed");
                        self.remove_peer(&peer_bin);
                        return Ok(());
                    }
                },
            };
        }

        // now begin sending/receiving ice candidates
        loop {
            futures::select! {
                offer = offer_stream.select_next_some() => {
                    debug!("set-local-description (re-negotiation)");

                    ws_sender
                        .send(ws::Message::text(serde_json::to_string(&offer)?))
                        .await?;
                },

                json_msg = ice_candidate_stream.select_next_some() => {
                    ws_sender
                        .send(ws::Message::text(serde_json::to_string(&json_msg)?))
                        .await?;
                },

                total_bytes_sent = total_bytes_sent_stream.select_next_some() => {
                    let mut stats = self.stats.lock().await;
                    stats.update_client_stats(
                        addr,
                        total_bytes_sent as usize,
                    );
                },

                message = webrtcbin_bus_stream.select_next_some() => {
                    if let MessageView::PropertyNotify(notify) = message.view() {
                        if let (_object, key, Some(value)) = notify.get() {
                            if key == "connection-state" {
                                let state = value.get::<gstreamer_webrtc::WebRTCPeerConnectionState>().unwrap();
                                // gotta spawn because weird Send *mut c_void error??
                                let weak_app = self.downgrade();
                                tokio::spawn(async move {
                                    let app = upgrade_weak!(weak_app);
                                    let mut stats = app.stats.lock().await;
                                    stats.on_client_state(addr, state);
                                });
                            }

                            // else if "ice-connection-state" => {
                            //     let state = value.get::<gstreamer_webrtc::WebRTCICEConnectionState>().unwrap();
                            //     debug!("!! {:?} {:?}", key, state);
                            // }
                        }
                    }

                    if self.handle_pipeline_message(message).await? {
                        self.remove_peer(&peer_bin);
                        return Ok(());
                    }
                },

                result = ws_receiver.select_next_some() => {
                    let message = result?;

                    if message.is_close() {
                        debug!("websocket closed");
                        self.remove_peer(&peer_bin);
                        return Ok(());
                    } else if let Ok(text) = message.to_str() {
                        let message = serde_json::from_str::<JsonMsg>(text)?;

                        match message {
                            JsonMsg::Sdp(sdp_answer) => {
                                let ret = SDPMessage::parse_buffer(sdp_answer.as_bytes())
                                    .map_err(|_| anyhow!("Failed to parse SDP answer"))?;
                                let description = WebRTCSessionDescription::new(
                                    WebRTCSDPType::Answer,
                                    ret,
                                );

                                let (sender, receiver) = oneshot::channel();
                                let promise = gstreamer::Promise::with_change_func(move |_reply| {
                                    sender.send(()).unwrap();
                                });

                                // debug!("set-remote-description");
                                webrtcbin
                                    .emit_by_name::<()>(
                                        "set-remote-description",
                                        &[&description, &promise],
                                    );

                                receiver.await.unwrap();
                            }

                            JsonMsg::Ice { sdp_m_line_index, candidate } => {
                                // debug!("add-ice-candidate");
                                // debug!("{}", candidate);

                                webrtcbin
                                .emit_by_name::<()>(
                                    "add-ice-candidate",
                                    &[&sdp_m_line_index, &candidate],
                                );
                            }

                            JsonMsg::Stats { total_packets_received, total_packets_lost } => {
                                let mut stats = self.stats.lock().await;
                                stats.update_remote_stats(
                                    addr,
                                    total_packets_received,
                                    total_packets_lost
                                );
                            }
                        }
                    }
                },
            };
        }
    }

    fn remove_peer(&self, peer_bin: &gstreamer::Bin) {
        // Block the tees shortly for removal
        let audio_tee_sinkpad = self.audio_tee.static_pad("sink").unwrap();
        let audio_block = audio_tee_sinkpad
            .add_probe(gstreamer::PadProbeType::BLOCK_DOWNSTREAM, |_pad, _info| {
                gstreamer::PadProbeReturn::Ok
            })
            .unwrap();

        let video_tee_sinkpad = self.video_tee.static_pad("sink").unwrap();
        let video_block = video_tee_sinkpad
            .add_probe(gstreamer::PadProbeType::BLOCK_DOWNSTREAM, |_pad, _info| {
                gstreamer::PadProbeReturn::Ok
            })
            .unwrap();

        // Release the tee pads and unblock

        if let Some(audio_sinkpad) = peer_bin.static_pad("audio_sink") {
            if let Some(audio_tee_srcpad) = audio_sinkpad.peer() {
                let _ = audio_tee_srcpad.unlink(&audio_sinkpad);
                self.audio_tee.release_request_pad(&audio_tee_srcpad);
            }
        }
        audio_tee_sinkpad.remove_probe(audio_block);

        if let Some(video_sinkpad) = peer_bin.static_pad("video_sink") {
            if let Some(video_tee_srcpad) = video_sinkpad.peer() {
                let _ = video_tee_srcpad.unlink(&video_sinkpad);
                self.video_tee.release_request_pad(&video_tee_srcpad);
            }
        }
        video_tee_sinkpad.remove_probe(video_block);

        // Then remove the peer bin gracefully from the pipeline
        let _ = self.pipeline.remove(peer_bin);
        let _ = peer_bin.set_state(gstreamer::State::Null);

        // self.audio_tee.set_state(gstreamer::State::Playing).unwrap();
        // self.video_tee.set_state(gstreamer::State::Playing).unwrap();
        // debug!("Setting Pipeline to Playing");
        // self.pipeline
        //     .set_state(gstreamer::State::Playing)
        //     .expect("Couldn't set pipeline to Playing");

        debug!("Removed peer");
    }

    /// returns true if should cleanup and return
    async fn handle_pipeline_message(&self, message: gstreamer::Message) -> Result<bool> {
        match message.view() {
            MessageView::StateChanged(message) => {
                if let Some(src) = message.src() {
                    if src.is::<gstreamer::Pipeline>()
                        && message.current() == gstreamer::State::Playing
                    {
                        let mut stats = self.stats.lock().await;
                        stats.set_state(stats::State::Streaming);
                    }
                }
            }

            MessageView::Eos(_) => {
                {
                    let mut stats = self.stats.lock().await;
                    stats.set_state(stats::State::StreamEnded);
                }
                return Ok(true);
            }

            MessageView::Error(err) => {
                bail!(
                    "Error from element {}: {} ({})",
                    err.src()
                        .map(|s| String::from(s.path_string()))
                        .unwrap_or_else(|| String::from("None")),
                    err.error(),
                    err.debug().unwrap_or_else(|| String::from("None").into()),
                );
            }

            MessageView::Warning(warning) => {
                warn!("Warning: \"{}\"", warning.debug().unwrap());
            }

            _ => {}
        }

        Ok(false)
    }
}

trait PipelineMake {
    fn make(&self, kind: &str) -> Result<Element>;
}
impl PipelineMake for Pipeline {
    fn make(&self, kind: &str) -> Result<Element> {
        let element = ElementFactory::make(kind).build()?;
        self.add(&element)?;
        Ok(element)
    }
}
