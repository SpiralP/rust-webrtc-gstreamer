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
};

const video = document.getElementById("video") as HTMLVideoElement;

const peerConnection = make("rtc");
const signallingConnection = new WebSocket(`ws://34.217.45.217:8080/`);

peerConnection.addEventListener("track", (event) => {
  console.log("ONTRACK");
  video.srcObject = event.streams[0];
  // video.play();
});
peerConnection.addEventListener("icecandidate", (event) => {
  console.log(event);

  const { candidate } = event;

  if (candidate == null) {
    console.log("ICE Candidate was null, done");
    return;
  }

  if (candidate.candidate.indexOf(".local") !== -1) {
    return;
  }

  (async () => {
    const msg: JsonMsg = {
      ice: {
        candidate: candidate.candidate,
        sdpMLineIndex: candidate.sdpMLineIndex,
      },
    };
    console.log("send ", msg);
    signallingConnection.send(JSON.stringify(msg));
  })();
});

signallingConnection.addEventListener("error", console.error);
signallingConnection.addEventListener("close", () => {
  console.log("closed");
});

signallingConnection.addEventListener("open", () => {
  console.log("connected");
});
signallingConnection.addEventListener("message", ({ data }) => {
  const msg = JSON.parse(data) as JsonMsg;

  (async () => {
    if (isJsonMsgSdp(msg)) {
      console.log("setRemoteDescription");
      console.log(msg.sdp);
      await peerConnection.setRemoteDescription({
        sdp: msg.sdp,
        type: "offer",
      });

      console.log("createAnswer");
      const answer = await peerConnection.createAnswer({
        // offerToReceiveAudio: true,
        offerToReceiveVideo: true,
      });

      console.log("setLocalDescription");
      await peerConnection.setLocalDescription(answer);
      console.log(peerConnection.localDescription);

      const answerMsg: JsonMsg = {
        sdp: peerConnection.localDescription.sdp,
      };
      console.log("send ", answerMsg);
      signallingConnection.send(JSON.stringify(answerMsg));
    } else if (isJsonMsgIce(msg)) {
      console.log("addIceCandidate");
      console.log(msg.ice.candidate);
      await peerConnection.addIceCandidate(msg.ice);
    }
  })();
});

// (async () => {

//   const a = make("A");
//   const channel = a.createDataChannel("data");

//   const b = make("B");

//   const offer = await a.createOffer();
//   await a.setLocalDescription(offer);
//   await b.setRemoteDescription(offer);

//   const answer = await b.createAnswer();
//   b.setLocalDescription(answer);
//   a.setRemoteDescription(answer);
// })();

function make(name: string): RTCPeerConnection {
  const connection = new RTCPeerConnection(rtcConfiguration);
  connection.addEventListener("negotiationneeded", () => {
    console.log(name, "onnegotiationneeded");
  });

  connection.addEventListener("icecandidate", ({ candidate }) => {
    console.log(name, "onicecandidate", candidate);
  });
  connection.addEventListener("icecandidateerror", (event) => {
    console.log(name, "onicecandidateerror", event.errorText);
  });
  connection.addEventListener("iceconnectionstatechange", () => {
    console.log(
      name,
      "oniceconnectionstatechange",
      connection.iceConnectionState
    );
  });
  connection.addEventListener("icegatheringstatechange", () => {
    console.log(
      name,
      "onicegatheringstatechange",
      connection.iceGatheringState
    );
  });
  connection.addEventListener("connectionstatechange", () => {
    console.log(name, "onconnectionstatechange", connection.connectionState);
  });
  connection.addEventListener("signalingstatechange", () => {
    console.log(name, "onsignalingstatechange", connection.signalingState);
  });

  connection.addEventListener("datachannel", (event) => {
    console.log(name, "ondatachannel", event.channel);
  });

  connection.addEventListener("track", (event) => {
    console.log(name, "ontrack", event.track);
  });

  return connection;
}
