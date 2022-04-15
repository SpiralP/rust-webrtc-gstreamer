import {
  isJsonMsgIce,
  isJsonMsgSdp,
  JsonMsg,
  JsonMsgIce,
  JsonMsgSdp,
  JsonMsgStats,
  sleep,
} from "./helpers";

const rtcConfiguration: RTCConfiguration = {
  iceServers: [{ urls: ["stun:stun.l.google.com:19302"] }],
  bundlePolicy: "max-bundle",
};

const video = document.getElementById("video") as HTMLVideoElement;

let retryTimeoutId: number | undefined;
let statsLoopIntervalId: number | undefined;
function main(delayUdp: boolean = false) {
  if (retryTimeoutId) {
    clearTimeout(retryTimeoutId);
    retryTimeoutId = undefined;
  }
  if (statsLoopIntervalId) {
    clearTimeout(statsLoopIntervalId);
    statsLoopIntervalId = undefined;
  }

  const peerConnection = new RTCPeerConnection(rtcConfiguration);
  // @ts-ignore
  window.peerConnection = peerConnection;
  const signalingConnection = new WebSocket(
    `${location.protocol === "https:" ? "wss" : "ws"}://${location.host}/ws`
  );

  peerConnection.addEventListener("track", (event) => {
    console.log("track", event.streams);
    video.srcObject = event.streams[0];
  });
  peerConnection.addEventListener("icecandidate", async (event) => {
    const { candidate } = event;

    if (candidate == null) {
      console.log("ICE Candidate was null, done");
      return;
    }

    if (
      candidate.candidate === "" ||
      // server won't know what to do with these
      candidate.candidate.indexOf(".local") !== -1
    ) {
      return;
    }
    console.log("local candidate", candidate.candidate);

    const msg: JsonMsgIce = {
      ice: {
        candidate: candidate.candidate,
        sdpMLineIndex: candidate.sdpMLineIndex!,
      },
    };
    signalingConnection.send(JSON.stringify(msg));
  });

  let timeout: number | undefined = undefined;
  peerConnection.addEventListener("connectionstatechange", () => {
    console.log("connectionState", peerConnection.connectionState);
    if (timeout != null) {
      clearTimeout(timeout);
      timeout = undefined;
    }

    if (peerConnection.connectionState === "connecting" && !delayUdp) {
      timeout = setTimeout(() => {
        if (peerConnection.connectionState !== "connecting") return;
        console.warn(
          "still in connecting state, retrying with UDP candidates delayed"
        );
        peerConnection.close();
        signalingConnection.onclose = null;
        signalingConnection.close();

        main(true);
      }, 3000);
    } else if (peerConnection.connectionState === "connected") {
      if (statsLoopIntervalId) {
        clearTimeout(statsLoopIntervalId);
        statsLoopIntervalId = undefined;
      }
      statsLoopIntervalId = setInterval(async () => {
        const stats = await peerConnection.getStats();
        let totalPacketsLost = 0;
        let totalPacketsReceived = 0;
        for (const [key, value] of Array.from(stats.entries())) {
          if (key.startsWith("RTCInboundRTP")) {
            totalPacketsLost += value.packetsLost;
            totalPacketsReceived += value.packetsReceived;
          }
        }

        const statsMsg: JsonMsgStats = {
          stats: {
            totalPacketsReceived,
            totalPacketsLost,
          },
        };
        signalingConnection.send(JSON.stringify(statsMsg));
      }, 1000);
    }
  });
  peerConnection.addEventListener("iceconnectionstatechange", () => {
    console.log("iceConnectionState", peerConnection.iceConnectionState);
  });
  peerConnection.addEventListener("signalingstatechange", () => {
    console.log("signalingState", peerConnection.signalingState);
  });
  peerConnection.addEventListener("negotiationneeded", () => {
    console.warn("negotiationneeded");
  });
  peerConnection.addEventListener("icecandidateerror", (event) => {
    console.warn("icecandidateerror", event);
  });

  signalingConnection.addEventListener("error", console.warn);
  signalingConnection.addEventListener("message", async ({ data }) => {
    const msg = JSON.parse(data) as JsonMsg;

    if (isJsonMsgSdp(msg)) {
      // msg.sdp = msg.sdp.replace(
      //   /(m=video.*\r\n)/g,
      //   "$1a=fmtp:96 max-fs=12288;max-fr=60\r\n"
      // );
      console.log("setRemoteDescription", msg.sdp);
      await peerConnection.setRemoteDescription({
        sdp: msg.sdp,
        type: "offer",
      });

      const answer = await peerConnection.createAnswer();

      console.log("setLocalDescription", answer.sdp);
      await peerConnection.setLocalDescription(answer);

      const answerMsg: JsonMsgSdp = {
        sdp: answer.sdp!,
      };
      signalingConnection.send(JSON.stringify(answerMsg));
    } else if (isJsonMsgIce(msg)) {
      if (delayUdp && msg.ice.candidate.indexOf(" UDP ") !== -1) {
        // fix for chrome on localhost connecting to udp first and
        // causing the video to hang, so we let TCP go first
        await sleep(2000);
      }

      console.log("remote candidate", msg.ice.candidate);
      await peerConnection.addIceCandidate(msg.ice);
    }
  });

  signalingConnection.addEventListener("open", () => {
    console.log("WebSocket connected");
  });
  signalingConnection.onclose = () => {
    if (statsLoopIntervalId) {
      clearTimeout(statsLoopIntervalId);
      statsLoopIntervalId = undefined;
    }

    console.log("WebSocket closed, retrying in a few seconds");
    if (retryTimeoutId) {
      clearTimeout(retryTimeoutId);
      retryTimeoutId = undefined;
    }
    retryTimeoutId = setTimeout(() => {
      peerConnection.close();

      main(delayUdp);
    }, 10000);
  };
}

main(
  location.hostname === "localhost" ||
    location.hostname.startsWith("127.0.0.") ||
    location.hostname.startsWith("10.0.0.") ||
    location.hostname.startsWith("192.168.1.")
);
