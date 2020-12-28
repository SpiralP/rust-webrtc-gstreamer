mod app;
mod macros;
mod types;

use crate::{app::App, types::Args};
use anyhow::*;

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

    let registry = gst::Registry::get();
    for n in &needed {
        if registry.find_plugin(n).is_none() {
            bail!("Missing plugin: {:?}", n);
        }
    }

    Ok(())
}

#[async_std::main]
#[paw::main]
async fn main(args: Args) -> Result<()> {
    // Initialize GStreamer first
    gst::init()?;
    check_plugins()?;

    // Create our application state
    let app = App::new(args)?;

    // All good, let's run our message loop
    app.run().await?;

    Ok(())
}
