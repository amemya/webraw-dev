"use client";

import dynamic from "next/dynamic";

// Dynamic import to avoid SSR for WASM + WebGPU components
const Editor = dynamic(() => import("@/components/Editor"), {
  ssr: false,
  loading: () => (
    <div style={{
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      height: "100vh",
      background: "#0d0d0f",
      color: "#6366f1",
      fontFamily: "Inter, sans-serif",
      fontSize: "16px",
    }}>
      <div style={{ textAlign: "center" }}>
        <div style={{
          width: "36px",
          height: "36px",
          border: "3px solid #2a2a34",
          borderTopColor: "#6366f1",
          borderRadius: "50%",
          animation: "spin 0.8s linear infinite",
          margin: "0 auto 16px",
        }} />
        RAW Processor を読み込み中...
      </div>
    </div>
  ),
});

export default function Home() {
  return <Editor />;
}
