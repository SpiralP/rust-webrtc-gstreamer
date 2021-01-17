use gst_webrtc::WebRTCPeerConnectionState;
use std::{
    collections::HashMap,
    io::{stderr, Stderr, Write},
    net::SocketAddr,
    os::raw::c_int,
    time::Instant,
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
pub struct ClientStats {
    state: WebRTCPeerConnectionState,
    total_bytes_sent: u64,
    total_packets_received: u64,
    total_packets_lost: c_int,
}
impl ClientStats {
    pub fn new() -> Self {
        Self {
            state: WebRTCPeerConnectionState::New,
            total_bytes_sent: 0,
            total_packets_received: 0,
            total_packets_lost: 0,
        }
    }
}

#[derive(Debug)]
pub struct Stats {
    stderr: Stderr,
    state: State,
    clients: HashMap<SocketAddr, ClientStats>,
    last_print: Instant,
    last_total_bytes: u64,
}
impl Stats {
    pub fn new() -> Self {
        Self {
            stderr: stderr(),
            state: State::Idle,
            clients: Default::default(),
            last_print: Instant::now(),
            last_total_bytes: 0,
        }
    }

    pub fn print(&mut self) {
        let now = Instant::now();
        let mut total_bytes = 0;
        for client in self.clients.values() {
            total_bytes += client.total_bytes_sent;
        }

        let secs = (now - self.last_print).as_secs_f32();

        let status = format!(
            "{:?} - {} clients - sending {:.1} KiB/s",
            self.state,
            self.clients.len(),
            ((total_bytes - self.last_total_bytes) as f32 / secs) / 1024.0
        );
        write!(
            self.stderr,
            "{}{}{}{}{}",
            HIDE_CURSOR, MOVE_TO_START_OF_LINE, ERASE_WHOLE_LINE, status, SHOW_CURSOR
        )
        .unwrap();
        self.stderr.flush().unwrap();

        self.last_total_bytes = total_bytes;
        self.last_print = now;
    }

    pub fn set_state(&mut self, state: State) {
        info!("{:?}", state);
        self.state = state;
    }

    pub fn on_client_connected(&mut self, addr: SocketAddr) {
        info!("client connected: {:?}", addr);
        self.clients.insert(addr, ClientStats::new());
    }

    pub fn on_client_disconnected(&mut self, addr: SocketAddr) {
        info!("client disconnected: {:?}", addr);
        self.clients.remove(&addr);
    }

    pub fn on_client_state(&mut self, addr: SocketAddr, state: WebRTCPeerConnectionState) {
        info!("client {:?} {:?}", addr, state);
        if let Some(client) = self.clients.get_mut(&addr) {
            client.state = state;
        }
    }

    pub fn update_client_stats(
        &mut self,
        addr: SocketAddr,
        total_bytes_sent: u64,
        total_packets_received: u64,
        total_packets_lost: c_int,
    ) {
        // println!("{} {}", total_packets_received, total_packets_lost);
        if let Some(client) = self.clients.get_mut(&addr) {
            client.total_bytes_sent = total_bytes_sent;
            client.total_packets_received = total_packets_received;
            client.total_packets_lost = total_packets_lost;
        }
    }
}
