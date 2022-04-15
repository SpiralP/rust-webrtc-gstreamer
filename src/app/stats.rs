use gst_webrtc::WebRTCPeerConnectionState;
use speedometer::Speedometer;
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
pub struct ClientStats {
    state: WebRTCPeerConnectionState,
    total_bytes_sent: usize,
    total_packets_received: usize,
    total_packets_lost: usize,
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
    speed_bytes: Speedometer,
    last_total_bytes: usize,
    speed_loss: Speedometer,
    last_total_packets_received: usize,
    last_total_packets_lost: usize,
}
impl Stats {
    pub fn new() -> Self {
        Self {
            stderr: stderr(),
            state: State::Idle,
            clients: Default::default(),
            speed_bytes: Default::default(),
            last_total_bytes: 0,
            speed_loss: Default::default(),
            last_total_packets_received: 0,
            last_total_packets_lost: 0,
        }
    }

    pub fn print(&mut self) {
        let mut total_bytes_sent = 0;
        let mut total_packets_received = 0;
        let mut total_packets_lost = 0;
        for client in self.clients.values() {
            total_bytes_sent += client.total_bytes_sent;
            total_packets_received += client.total_packets_received;
            total_packets_lost += client.total_packets_lost;
        }

        let diff_total_bytes_sent = total_bytes_sent.saturating_sub(self.last_total_bytes);
        self.last_total_bytes = total_bytes_sent;
        self.speed_bytes.entry(diff_total_bytes_sent);

        let diff_total_packets_received =
            total_packets_received.saturating_sub(self.last_total_packets_received);
        self.last_total_packets_received = total_packets_received;

        let diff_total_packets_lost =
            total_packets_lost.saturating_sub(self.last_total_packets_lost);
        self.last_total_packets_lost = total_packets_received;

        let loss = if diff_total_packets_received == 0 {
            0.0
        } else {
            diff_total_packets_lost as f32 / diff_total_packets_received as f32
        };

        self.speed_loss.entry((loss * 100.0) as usize);

        let status = format!(
            "{:?} - {} clients - sending {} KiB/s - {}% loss",
            self.state,
            self.clients.len(),
            self.speed_bytes.measure().unwrap() / 1024,
            self.speed_loss.measure().unwrap()
        );
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

    pub fn update_client_stats(&mut self, addr: SocketAddr, total_bytes_sent: usize) {
        // println!("{} {}", total_packets_received, total_packets_lost);
        if let Some(client) = self.clients.get_mut(&addr) {
            client.total_bytes_sent = total_bytes_sent;
        }
    }

    pub fn update_remote_stats(
        &mut self,
        addr: SocketAddr,
        total_packets_received: usize,
        total_packets_lost: usize,
    ) {
        if let Some(client) = self.clients.get_mut(&addr) {
            client.total_packets_received = total_packets_received;
            client.total_packets_lost = total_packets_lost;
        }
    }
}
