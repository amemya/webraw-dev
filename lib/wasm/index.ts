// lib/wasm/index.ts
// WASM module loader for the raw-processor crate

import type { DecodedImage } from "../types";

// eslint-disable-next-line @typescript-eslint/no-explicit-any
let wasmModule: any = null;

export async function initWasm(): Promise<void> {
    if (wasmModule) return;

    try {
        const wasm = await import("../../crate/pkg/raw_processor");
        await wasm.default();
        wasmModule = wasm;
    } catch (e) {
        throw new Error(`Failed to load WASM module: ${e}`);
    }
}

export function isWasmReady(): boolean {
    return wasmModule !== null;
}

export async function decodeRaw(fileData: ArrayBuffer): Promise<DecodedImage> {
    await initWasm();

    const uint8 = new Uint8Array(fileData);
    const result = wasmModule.decode_raw(uint8);

    return {
        pixels: result.pixels as Float32Array,
        width: result.width as number,
        height: result.height as number,
        metadata: result.metadata,
    };
}

export function getVersion(): string {
    if (!wasmModule) throw new Error("WASM module not loaded");
    return wasmModule.version();
}
