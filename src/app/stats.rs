use gst_webrtc::WebRTCPeerConnectionState;
use std::{
    collections::HashMap,
    io::{stderr, Stderr, Write},
    net::SocketAddr,
};
use tracing::*;

const ERASE_WHOLE_LINE: &str = "\x1B[2K";
const MOVE_TO_START_OF_LINE: &str = "\r";
const SHOW_CURSOR: &str = "\x1B[?25h";
const HIDE_CURSOR: &str = "\x1B[?25l";

#[derive(Debug)]
pub enum State {
    Idle,
    WaitingForStream,
    Streaming,
    StreamEnded,
}

#[derive(Debug)]
pub struct Stats {
    stderr: Stderr,
    state: State,
    clients: HashMap<SocketAddr, WebRTCPeerConnectionState>,
}
impl Stats {
    pub fn new() -> Self {
        Self {
            stderr: stderr(),
            state: State::Idle,
            clients: Default::default(),
        }
    }

    pub fn print(&mut self) {
        let status = format!("{:?} - {} clients", self.state, self.clients.len());
        write!(
            self.stderr,
            "{}{}{}{}{}",
            HIDE_CURSOR, MOVE_TO_START_OF_LINE, ERASE_WHOLE_LINE, status, SHOW_CURSOR
        )
        .unwrap();
        self.stderr.flush().unwrap();
    }

    pub fn set_state(&mut self, state: State) {
        info!("{:?}", state);
        self.state = state;
    }

    pub fn on_client_connected(&mut self, addr: SocketAddr) {
        info!("client connected: {:?}", addr);
        self.clients.insert(addr, WebRTCPeerConnectionState::New);
    }

    pub fn on_client_disconnected(&mut self, addr: SocketAddr) {
        info!("client disconnected: {:?}", addr);
        self.clients.remove(&addr);
    }

    pub fn on_client_state(&mut self, addr: SocketAddr, state: WebRTCPeerConnectionState) {
        info!("client {:?} {:?}", addr, state);
        // self.clients.insert(addr, state);
    }
}
