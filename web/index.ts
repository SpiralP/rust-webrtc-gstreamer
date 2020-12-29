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
  iceServers: [{ urls: "stun:stun.l.google.com:19302" }],
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
    candidate.candidate.indexOf(".local") !== -1
  ) {
    return;
  }
  console.log("local candidate", candidate);

  const msg: JsonMsgIce = {
    ice: {
      candidate: candidate.candidate,
      sdpMLineIndex: candidate.sdpMLineIndex,
    },
  };
  signalingConnection.send(JSON.stringify(msg));
});
peerConnection.addEventListener("signalingstatechange", (event) => {
  console.log("signalingstatechange", peerConnection.signalingState);
});

signalingConnection.addEventListener("error", console.error);
signalingConnection.addEventListener("message", async ({ data }) => {
  const msg = JSON.parse(data) as JsonMsg;

  if (isJsonMsgSdp(msg)) {
    console.log("setRemoteDescription", msg.sdp);
    await peerConnection.setRemoteDescription({
      sdp: msg.sdp,
      type: "offer",
    });

    const answer = await peerConnection.createAnswer();

    console.log("setLocalDescription", answer.sdp);
    await peerConnection.setLocalDescription(answer);

    const answerMsg: JsonMsgSdp = {
      sdp: peerConnection.localDescription.sdp,
    };
    signalingConnection.send(JSON.stringify(answerMsg));
  } else if (isJsonMsgIce(msg)) {
    console.log("remote candidate", msg.ice.candidate);
    await peerConnection.addIceCandidate(msg.ice);
  }
});
