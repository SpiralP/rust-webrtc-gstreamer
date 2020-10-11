const rtcConfiguration = {
  iceServers: [{ urls: "stun:stun.l.google.com:19302" }],
};

const peerConnection = new RTCPeerConnection(rtcConfiguration);
const signallingConnection = new WebSocket("ws://127.0.0.1:2222/");

peerConnection.ondatachannel = () => console.log("ONDATACHANNEL");
peerConnection.ontrack = () => console.log("ONTRACK");
peerConnection.onicecandidate = (event) => {
  if (event.candidate == null) {
    console.log("ICE Candidate was null, done");
    return;
  }

  console.log("send ", event.candidate.candidate);
  signallingConnection.send(JSON.stringify({ ice: event.candidate }));
};

signallingConnection.addEventListener("error", console.error);
signallingConnection.addEventListener("close", () => {
  console.log("closed");
});

signallingConnection.addEventListener("open", (event) => {
  console.log("connected");
});
signallingConnection.addEventListener("message", ({ data }) => {
  const o = JSON.parse(data);
  console.log(o);

  if (o.sdp != null) {
    (async () => {
      console.log("setRemoteDescription");
      await peerConnection.setRemoteDescription({
        sdp: o.sdp,
        type: "offer",
      });

      console.log("createAnswer");
      const answer = await peerConnection.createAnswer();

      console.log("setLocalDescription");
      await peerConnection.setLocalDescription(answer);
    })();
  }
});

// (async () => {
//   function make(name: string): RTCPeerConnection {
//     const connection = new RTCPeerConnection(rtcConfiguration);
//     connection.onnegotiationneeded = () => {
//       console.log(name, "onnegotiationneeded");
//     };

//     connection.onicecandidate = ({ candidate }) => {
//       console.log(name, "onicecandidate", candidate);
//     };
//     connection.onicecandidateerror = () => {
//       console.log(name, "onicecandidateerror");
//     };
//     connection.oniceconnectionstatechange = () => {
//       console.log(
//         name,
//         "oniceconnectionstatechange",
//         connection.iceConnectionState
//       );
//     };
//     connection.onicegatheringstatechange = () => {
//       console.log(
//         name,
//         "onicegatheringstatechange",
//         connection.iceGatheringState
//       );
//     };
//     connection.onconnectionstatechange = () => {
//       console.log(name, "onconnectionstatechange");
//     };
//     connection.onsignalingstatechange = () => {
//       console.log(name, "onsignalingstatechange", connection.signalingState);
//     };

//     connection.ondatachannel = () => {
//       console.log(name, "ondatachannel");
//     };

//     connection.onidentityresult = () => {
//       console.log(name, "onidentityresult");
//     };

//     connection.ondatachannel = () => {
//       console.log(name, "ondatachannel");
//     };

//     return connection;
//   }

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
