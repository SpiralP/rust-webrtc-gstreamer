use crate::{types::JsonMsg, Args};
use anyhow::{anyhow, bail};
use async_std::task;
use async_tungstenite::tungstenite;
use futures::stream::StreamExt;
use gstreamer::{
    gst_element_error, message::MessageView, prelude::*, Caps, Element, ElementFactory, Pipeline,
    State,
};
use std::sync::{Arc, Mutex, Weak};
use tungstenite::Message as WsMessage;

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

// Strong reference to our application state
#[derive(Clone)]
pub struct App(Arc<AppInner>);

// Weak reference to our application state
#[derive(Clone)]
struct AppWeak(Weak<AppInner>);

// Actual application state
pub struct AppInner {
    pub args: Args,
    pub pipeline: gstreamer::Pipeline,
    pub webrtcbin: gstreamer::Element,
    pub candidates: Mutex<Vec<WsMessage>>,
    pub offer: Mutex<Option<String>>,
}

// To be able to access the App's fields directly
impl std::ops::Deref for App {
    type Target = AppInner;

    fn deref(&self) -> &AppInner {
        &self.0
    }
}

impl AppWeak {
    // Try upgrading a weak reference to a strong one
    fn upgrade(&self) -> Option<App> {
        self.0.upgrade().map(App)
    }
}

impl App {
    // Downgrade the strong reference to a weak reference
    fn downgrade(&self) -> AppWeak {
        AppWeak(Arc::downgrade(&self.0))
    }

    // webrtcbin bundle-policy=max-bundle name=sendrecv stun-server=stun://stun.l.google.com:19302 \

    pub async fn new(args: Args) -> Result<Self, anyhow::Error> {
        // Create the GStreamer pipeline
        let pipeline = Pipeline::new(None);

        macro_rules! make {
            ($kind:expr, $name:expr) => {{
                let a = ElementFactory::make($kind, Some($name)).unwrap();
                pipeline.add(&a).unwrap();
                a
            }};

            ($kind:expr) => {{
                let a = ElementFactory::make($kind, None).unwrap();
                pipeline.add(&a).unwrap();
                a
            }};
        }

        fn link(elements: &[&Element], last: Element) -> Element {
            let mut prev: Option<&Element> = None;
            for &e in elements {
                if let Some(prev) = prev {
                    prev.link(e).unwrap();
                }
                prev = Some(e);
            }
            if let Some(prev) = prev {
                prev.link(&last).unwrap();
            }
            last
        }

        // videotestsrc -> videoconvert -> vp8enc -> rtpvp8pay -> tee -> { queue -> filesink }
        let rtpvp8pay = link(
            &[
                &{
                    let video_src = make!("videotestsrc");
                    video_src.set_property("is-live", &true).unwrap();
                    video_src
                },
                &make!("videoconvert"),
                &{
                    let vp8enc = make!("vp8enc");
                    vp8enc.set_property("deadline", &1i64).unwrap();
                    vp8enc
                },
            ],
            make!("rtpvp8pay"),
        );

        let tee = make!("tee");
        rtpvp8pay
            .link_filtered(
                &tee,
                Some(&Caps::new_simple(
                    "application/x-rtp",
                    &[
                        ("media", &"video"),
                        ("encoding-name", &"VP8"),
                        ("payload", &96),
                    ],
                )),
            )
            .unwrap();

        {
            let filesink = link(&[&tee, &make!("queue")], make!("filesink"));
            filesink
                .set_property("location", &"target/ag.webm")
                .unwrap();
        }
        {
            let filesink = link(&[&tee, &make!("queue")], make!("filesink"));
            filesink
                .set_property("location", &"target/ag2.webm")
                .unwrap();
        }

        // let webrtcbin = make!("webrtcbin");
        // webrtcbin
        //     .set_property("bundle-policy", &WebRTCBundlePolicy::MaxBundle)
        //     .unwrap();
        // webrtcbin
        //     .set_property("stun-server", &"stun://stun.l.google.com:19302")
        //     .unwrap();

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

        let bus = pipeline.get_bus().unwrap();
        let mut send_gst_msg_rx = bus.stream();

        pipeline.call_async(|pipeline| {
            println!("starting");
            pipeline
                .set_state(gstreamer::State::Playing)
                .expect("Couldn't set pipeline to Playing");
        });

        let handle = task::spawn(async move {
            while let Some(message) = send_gst_msg_rx.next().await {
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
        });

        // std::thread::sleep_ms(3000);

        // println!("adding more!");
        // {
        //     let queue = make!("queue");
        //     let filesink = link(&[&tee, &queue], make!("filesink"));
        //     filesink
        //         .set_property("location", &"target/ag3.webm")
        //         .unwrap();

        //     queue.set_state(gstreamer::State::Playing).unwrap();
        //     filesink.set_state(gstreamer::State::Playing).unwrap();
        // }

        handle.await.unwrap();

        unimplemented!();

        // // Connect to on-negotiation-needed to handle sending an Offer
        // let app_clone = app.downgrade();
        // app.webrtcbin
        //     .connect("on-negotiation-needed", false, move |values| {
        //         let _webrtc = values[0].get::<gstreamer::Element>().unwrap();

        //         let app = upgrade_weak!(app_clone, None);
        //         if let Err(err) = app.on_negotiation_needed() {
        //             gst_element_error!(
        //                 app.pipeline,
        //                 gstreamer::LibraryError::Failed,
        //                 ("Failed to negotiate: {:?}", err)
        //             );
        //         }

        //         None
        //     })
        //     .unwrap();

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
    }

    // Whenever webrtcbin tells us that (re-)negotiation is needed, simply ask
    // for a new offer SDP from webrtcbin without any customization and then
    // asynchronously send it to the peer via the WebSocket connection
    fn on_negotiation_needed(&self) -> Result<(), anyhow::Error> {
        println!("starting negotiation");

        let app_clone = self.downgrade();
        let promise = gstreamer::Promise::with_change_func(move |reply| {
            let app = upgrade_weak!(app_clone);

            if let Err(err) = app.on_offer_created(reply) {
                gst_element_error!(
                    app.pipeline,
                    gstreamer::LibraryError::Failed,
                    ("Failed to send SDP offer: {:?}", err)
                );
            }
        });

        self.webrtcbin
            .emit("create-offer", &[&None::<gstreamer::Structure>, &promise])
            .unwrap();

        Ok(())
    }

    // Once webrtcbin has create the offer SDP for us, handle it by sending it to the peer via the
    // WebSocket connection
    fn on_offer_created(
        &self,
        reply: Result<Option<&gstreamer::StructureRef>, gstreamer::PromiseError>,
    ) -> Result<(), anyhow::Error> {
        let reply = match reply {
            Ok(Some(reply)) => reply,
            Ok(None) => {
                bail!("Offer creation future got no reponse");
            }
            Err(err) => {
                bail!("Offer creation future got error reponse: {:?}", err);
            }
        };

        let offer = reply
            .get_value("offer")
            .unwrap()
            .get::<gstreamer_webrtc::WebRTCSessionDescription>()
            .expect("Invalid argument")
            .unwrap();

        self.webrtcbin
            .emit(
                "set-local-description",
                &[&offer, &None::<gstreamer::Promise>],
            )
            .unwrap();

        let sdp_text = offer.get_sdp().as_text().unwrap();
        println!("setting default SDP offer");
        println!("{}", sdp_text);

        let offer = &mut *self.offer.lock().unwrap();
        *offer = Some(sdp_text);

        Ok(())
    }

    // Handle incoming SDP answers from the peer
    fn handle_sdp(&self, sdp: &str) -> Result<(), anyhow::Error> {
        println!("Received answer:");
        println!("{}", sdp);

        let ret = gstreamer_sdp::SDPMessage::parse_buffer(sdp.as_bytes())
            .map_err(|_| anyhow!("Failed to parse SDP answer"))?;
        let answer = gstreamer_webrtc::WebRTCSessionDescription::new(
            gstreamer_webrtc::WebRTCSDPType::Answer,
            ret,
        );

        self.webrtcbin
            .emit(
                "set-remote-description",
                &[&answer, &None::<gstreamer::Promise>],
            )
            .unwrap();

        Ok(())
    }

    // Handle incoming ICE candidates from the peer by passing them to webrtcbin
    fn handle_ice(&self, sdp_m_line_index: u32, candidate: &str) -> Result<(), anyhow::Error> {
        println!("recv {}", candidate);
        self.webrtcbin
            .emit("add-ice-candidate", &[&sdp_m_line_index, &candidate])
            .unwrap();

        Ok(())
    }

    // Asynchronously send ICE candidates to the peer via the WebSocket connection as a JSON
    // message
    fn on_ice_candidate(&self, mlineindex: u32, candidate: String) -> Result<(), anyhow::Error> {
        println!("{} {}", mlineindex, candidate);

        let candidates = &mut *self.candidates.lock().unwrap();
        candidates.push(WsMessage::Text(
            serde_json::to_string(&JsonMsg::Ice {
                candidate,
                sdp_m_line_index: mlineindex,
            })
            .unwrap(),
        ));

        Ok(())
    }
}

// Make sure to shut down the pipeline when it goes out of scope
// to release any system resources
impl Drop for AppInner {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(State::Null);
    }
}
