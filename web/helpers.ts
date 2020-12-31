export type JsonMsg = JsonMsgSdp | JsonMsgIce;

export interface JsonMsgSdp {
  sdp: string;
}

export interface JsonMsgIce {
  ice: {
    candidate: string;
    sdpMLineIndex: number;
  };
}

export function isJsonMsgSdp(msg: JsonMsg): msg is JsonMsgSdp {
  return (msg as JsonMsgSdp).sdp !== undefined;
}

export function isJsonMsgIce(msg: JsonMsg): msg is JsonMsgIce {
  return (msg as JsonMsgIce).ice !== undefined;
}

export async function sleep(ms: number) {
  await new Promise((resolve) => setTimeout(resolve, ms));
}
