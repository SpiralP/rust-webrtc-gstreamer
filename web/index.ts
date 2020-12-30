type JsonMsg = JsonMsgSdp | JsonMsgIce;

interface JsonMsgSdp {
  sdp: string;
}

interface JsonMsgIce {
  ice: {
    candidate: string;
    sdpMLineIndex: number;
  };
}

function isJsonMsgSdp(msg: JsonMsg): msg is JsonMsgSdp {
  return (msg as JsonMsgSdp).sdp !== undefined;
}

function isJsonMsgIce(msg: JsonMsg): msg is JsonMsgIce {
  return (msg as JsonMsgIce).ice !== undefined;
}

const rtcConfiguration: RTCConfiguration = {
  iceServers: [{ urls: ["stun:stun.l.google.com:19302"] }],
  bundlePolicy: "max-bundle",
};

const video = document.getElementById("video") as HTMLVideoElement;

const peerConnection = new RTCPeerConnection(rtcConfiguration);
// @ts-ignore
global.peerConnection = peerConnection;
const signalingConnection = new WebSocket(`ws://${location.host}/ws`);

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
      sdpMLineIndex: candidate.sdpMLineIndex,
    },
  };
  signalingConnection.send(JSON.stringify(msg));
});
peerConnection.addEventListener("connectionstatechange", () => {
  console.log("connectionState", peerConnection.connectionState);
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

signalingConnection.addEventListener("error", console.error);
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
      sdp: answer.sdp,
    };
    signalingConnection.send(JSON.stringify(answerMsg));
  } else if (isJsonMsgIce(msg)) {
    if (
      (location.hostname === "localhost" ||
        location.hostname === "127.0.0.1") &&
      msg.ice.candidate.indexOf(" UDP ") !== -1
    ) {
      // fix for chrome on localhost connecting to udp first, but not being able to send any data
      // so we let TCP go first
      await sleep(2000);
    }

    console.log("remote candidate", msg.ice.candidate);
    await peerConnection.addIceCandidate(msg.ice);
  }
});

async function sleep(ms: number) {
  await new Promise((resolve) => setTimeout(resolve, ms));
}
