"use client";

import { WsConnectionState } from "@/types";
import { useEffect, useRef, useState } from "react";

export default function Home() {
  const wsRef = useRef<WebSocket | null>(null);
  const wsUrl = "ws://localhost:8000/ws";
  const [connected, setConnected] = useState<WsConnectionState>(WsConnectionState.NOT_CONNECTED);
  const [messages, setMessages] = useState<string[]>([]);

  useEffect(() => {
    if (!wsRef.current) {
      wsRef.current = new WebSocket(wsUrl);
      setConnected(WsConnectionState.CONNECTING)
      wsRef.current.onopen = () => setConnected(WsConnectionState.CONNECTED);
    }
  }, []);

  useEffect(() => {
    const ws = wsRef.current;
    if (!ws) return;
    ws.onmessage = (msgEvent: MessageEvent<string>) => {
      const msg = msgEvent.data;
      console.log("got message: ", msg);
    };
  }, [connected]);

  return (
    <div className="">
      <h1 className="text-2xl flex-col">
        <div>
          {
            connected === WsConnectionState.CONNECTED ? "CONNECTED" : "NOT-CONNECTED"
          }
        </div>

        <div>
          {
            connected === WsConnectionState.CONNECTING ? "CONNECTING" : ""
          }
        </div>

        <div>
          {
            messages.map(msg => <p>{msg}</p>)
          }
        </div>
      </h1>
    </div>
  );
}
