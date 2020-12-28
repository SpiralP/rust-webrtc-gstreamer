mod app;
mod logger;
mod macros;

use self::app::{App, Args};
use anyhow::*;
use tracing::*;

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

#[paw::main]
#[tokio::main]
async fn main(args: Args) -> Result<()> {
    logger::initialize(true, false);

    debug!("{:#?}", args);

    // Initialize GStreamer first
    gst::init()?;
    check_plugins()?;

    // Create our application state
    let app = App::new(args)?;

    // All good, let's run our message loop
    app.run().await?;

    Ok(())
}
