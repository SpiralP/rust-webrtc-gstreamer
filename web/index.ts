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
const signallingConnection = new WebSocket(`ws://spiralp.uk.to:61234/`);

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
  console.log("local candidate", candidate);

  if (
    candidate.candidate === "" ||
    candidate.candidate.indexOf(".local") !== -1
  ) {
    return;
  }

  const msg: JsonMsgIce = {
    ice: {
      candidate: candidate.candidate,
      sdpMLineIndex: candidate.sdpMLineIndex,
    },
  };
  signallingConnection.send(JSON.stringify(msg));
});
peerConnection.addEventListener("signalingstatechange", (event) => {
  console.log("signalingstatechange", peerConnection.signalingState);
});

signallingConnection.addEventListener("error", console.error);
signallingConnection.addEventListener("message", async ({ data }) => {
  const msg = JSON.parse(data) as JsonMsg;

  if (isJsonMsgSdp(msg)) {
    console.log("setRemoteDescription", msg.sdp);
    await peerConnection.setRemoteDescription({
      sdp: msg.sdp,
      type: "offer",
    });

    const answer = await peerConnection.createAnswer({
      offerToReceiveAudio: true,
      offerToReceiveVideo: true,
    });

    console.log("setLocalDescription", answer.sdp);
    await peerConnection.setLocalDescription(answer);

    const answerMsg: JsonMsgSdp = {
      sdp: peerConnection.localDescription.sdp,
    };
    signallingConnection.send(JSON.stringify(answerMsg));
  } else if (isJsonMsgIce(msg)) {
    console.log("remote candidate", msg.ice.candidate);
    await peerConnection.addIceCandidate(msg.ice);
  }
});
