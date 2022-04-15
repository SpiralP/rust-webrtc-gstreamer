mod app;
mod logger;
mod macros;

use self::app::{App, Args};
use anyhow::*;
use std::{env, time::Duration};
use tracing::*;

// Check if all GStreamer plugins we require are available
fn check_plugins() -> Result<()> {
    let needed = [
        "coreelements",
        "webrtc",
        "rtpmanager",
        "tcp",
        "mpegtsdemux",
        "videoparsersbad",
        "libav",
        "videoconvert",
        "videoscale",
        "videorate",
        "vpx",
        "rtp",
        "audioparsers",
        "audioconvert",
        "audioresample",
        "opus",
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
    #[cfg(target_os = "windows")]
    if let Err(e) = ansi_term::enable_ansi_support() {
        eprintln!("enable_ansi_support: {}", e);
    }

    logger::initialize(
        cfg!(debug_assertions) || args.verbose > 0,
        args.verbose >= 2,
    );

    info!("{:#?}", args);

    if env::var("GST_DEBUG").is_err() {
        // show warnings
        env::set_var("GST_DEBUG", "*:2,GST_STATES:2,webrtcbin:2");
    }

    // Initialize GStreamer first
    gst::init()?;
    check_plugins()?;

    // Create our application state
    let (app, stats) = App::new(args)?;

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let mut stats = stats.lock().await;
            stats.print();
        }
    });

    // All good, let's run our message loop
    app.run().await?;

    Ok(())
}
