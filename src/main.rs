#![recursion_limit = "512"]
// to fix "a=fingerprint:sha-256 (null)"
// export OPENSSL_CONF=""
// a=candidate:1 1 UDP 2147483647 123.123.123.123 50007 typ host

mod app;
mod types;

use crate::{app::App, types::Args};
use anyhow::*;
use structopt::StructOpt;

// Check if all GStreamer plugins we require are available
fn check_plugins() -> Result<()> {
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
async fn main() -> Result<()> {
    // Initialize GStreamer first
    gstreamer::init()?;
    check_plugins()?;

    let args = Args::from_args();

    // Create our application state
    let app = App::new(args)?;

    // All good, let's run our message loop
    app.run().await?;

    Ok(())
}
