"use client";

import React, { useState, useCallback, useRef, useEffect } from "react";
import type { ProcessingParams, DecodedImage, RawMetadata } from "@/lib/types";
import { DEFAULT_PARAMS } from "@/lib/types";
import { decodeRaw } from "@/lib/wasm";
import { initGPUDevice, isWebGPUAvailable } from "@/lib/gpu/device";
import { ImageProcessor } from "@/lib/gpu/pipeline";
import FileDropZone from "./FileDropZone";
import Controls from "./Controls";
import Histogram from "./Histogram";

type Status = "idle" | "loading" | "processing" | "ready" | "error";

export default function Editor() {
    const [status, setStatus] = useState<Status>("idle");
    const [error, setError] = useState<string | null>(null);
    const [params, setParams] = useState<ProcessingParams>(DEFAULT_PARAMS);
    const [metadata, setMetadata] = useState<RawMetadata | null>(null);
    const [histogramData, setHistogramData] = useState<Uint8ClampedArray | null>(null);
    const [gpuAvailable, setGpuAvailable] = useState<boolean | null>(null);
    const [fileName, setFileName] = useState<string>("");

    // Single canvas ref — always mounted (hidden when not in use)
    const canvasRef = useRef<HTMLCanvasElement>(null);
    const processorRef = useRef<ImageProcessor | null>(null);
    const decodedImageRef = useRef<DecodedImage | null>(null);
    const processingTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

    // Check WebGPU availability on mount
    useEffect(() => {
        isWebGPUAvailable().then(setGpuAvailable);
    }, []);

    // Process image when params change
    const processImage = useCallback(async (newParams: ProcessingParams) => {
        const processor = processorRef.current;
        const canvas = canvasRef.current;
        if (!processor || !canvas) return;

        try {
            await processor.process(newParams);
            processor.render(canvas);

            const data = processor.readback();
            setHistogramData(data);
        } catch (e) {
            console.error("Processing error:", e);
        }
    }, []);

    // Handle param changes with debounce
    const handleParamsChange = useCallback(
        (newParams: ProcessingParams) => {
            setParams(newParams);

            if (processingTimeoutRef.current) {
                clearTimeout(processingTimeoutRef.current);
            }

            processingTimeoutRef.current = setTimeout(() => {
                processImage(newParams);
            }, 16); // ~60fps
        },
        [processImage]
    );

    // Handle file selection
    const handleFileSelected = useCallback(async (file: File) => {
        setStatus("loading");
        setError(null);
        setFileName(file.name);

        try {
            // Read file as ArrayBuffer
            const arrayBuffer = await file.arrayBuffer();

            // Decode RAW via WASM
            console.log("[RAW Processor] Decoding RAW file...");
            const decoded = await decodeRaw(arrayBuffer);
            console.log(`[RAW Processor] Decoded: ${decoded.width}x${decoded.height}, pixels: ${decoded.pixels.length}`);
            console.log("[RAW Processor] Metadata:", JSON.stringify(decoded.metadata, null, 2));
            console.log("[RAW Processor] WB coeffs:", decoded.metadata.wb_coeffs);
            console.log("[RAW Processor] CFA pattern:", decoded.metadata.cfa_pattern);
            console.log("[RAW Processor] xyz_to_cam (raw):", decoded.metadata.xyz_to_cam_raw);
            console.log("[RAW Processor] cam_to_srgb (computed):", decoded.metadata.color_matrix);
            console.log("[RAW Processor] First 12 pixel values (4 pixels RGB):",
                Array.from(decoded.pixels.slice(0, 12)).map(v => v.toFixed(4)));
            decodedImageRef.current = decoded;

            // Set up initial WB from camera metadata
            // Camera WB is already applied pre-demosaic in Rust.
            // GPU shader WB starts at identity; user adjusts via temperature/tint sliders.
            const initialParams: ProcessingParams = {
                ...DEFAULT_PARAMS,
                whiteBalance: [1.0, 1.0, 1.0],
            };

            setMetadata(decoded.metadata);
            setParams(initialParams);

            // Initialize WebGPU (device only — no canvas context needed for compute)
            console.log("[RAW Processor] Initializing WebGPU...");
            const device = await initGPUDevice();
            if (!device) {
                throw new Error("WebGPU initialization failed. Please use Chrome or Edge.");
            }

            // Create processor
            const processor = new ImageProcessor(device);
            await processor.init();
            console.log("[RAW Processor] Uploading image to GPU...");
            await processor.uploadImage(decoded.pixels, decoded.width, decoded.height);

            // Clean up previous processor
            processorRef.current?.destroy();
            processorRef.current = processor;

            // Process the image
            console.log("[RAW Processor] Processing...");
            setStatus("processing");
            await processor.process(initialParams);

            // Render to canvas (canvas ref is always mounted)
            const canvas = canvasRef.current;
            if (canvas) {
                processor.render(canvas);
                const histData = processor.readback();
                setHistogramData(histData);
            }

            console.log("[RAW Processor] Ready!");
            setStatus("ready");
        } catch (e) {
            const message = e instanceof Error ? e.message : String(e);
            setError(message);
            setStatus("error");
            console.error("[RAW Processor] Error:", e);
        }
    }, []);

    // Export to JPEG
    const handleExport = useCallback(async () => {
        const canvas = canvasRef.current;
        if (!canvas) return;

        try {
            const blob = await new Promise<Blob | null>((resolve) =>
                canvas.toBlob(resolve, "image/jpeg", 0.95)
            );
            if (!blob) return;

            const url = URL.createObjectURL(blob);
            const a = document.createElement("a");
            a.href = url;
            a.download = fileName.replace(/\.[^.]+$/, "") + "_processed.jpg";
            a.click();
            URL.revokeObjectURL(url);
        } catch (e) {
            console.error("Export error:", e);
        }
    }, [fileName]);

    // Reset params
    const handleReset = useCallback(() => {
        const decoded = decodedImageRef.current;
        if (!decoded) return;

        const resetParams: ProcessingParams = {
            ...DEFAULT_PARAMS,
            whiteBalance: [1.0, 1.0, 1.0],
        };
        setParams(resetParams);
        processImage(resetParams);
    }, [processImage]);

    // Cleanup
    useEffect(() => {
        return () => {
            processorRef.current?.destroy();
        };
    }, []);

    const hasImage = status !== "idle" && status !== "loading";

    return (
        <div className="editor">
            {/* Header */}
            <header className="editor__header">
                <h1 className="editor__title">
                    <span className="editor__title-icon">◈</span>
                    RAW Processor
                </h1>
                {metadata && (
                    <div className="editor__meta">
                        <span className="editor__meta-item">{metadata.make} {metadata.model}</span>
                        <span className="editor__meta-divider">|</span>
                        <span className="editor__meta-item">{metadata.width} × {metadata.height}</span>
                        <span className="editor__meta-divider">|</span>
                        <span className="editor__meta-item">{fileName}</span>
                    </div>
                )}
                {gpuAvailable === false && (
                    <div className="editor__warning">
                        ⚠ WebGPU が利用できません。Chrome または Edge をお使いください。
                    </div>
                )}
            </header>

            {/* Main content */}
            <div className="editor__body">
                {/* Canvas area */}
                <div className="editor__canvas-area">
                    {status === "idle" && (
                        <FileDropZone onFileSelected={handleFileSelected} isLoading={false} />
                    )}
                    {status === "loading" && (
                        <FileDropZone onFileSelected={handleFileSelected} isLoading={true} />
                    )}

                    {/* Canvas is always in the DOM but hidden when not needed */}
                    <div
                        className="editor__canvas-wrapper"
                        style={{ display: hasImage ? "flex" : "none" }}
                    >
                        <canvas ref={canvasRef} className="editor__canvas" />
                    </div>

                    {hasImage && (
                        <div className="editor__canvas-toolbar">
                            <label className="editor__reopen-btn" htmlFor="file-reopen">
                                別のファイルを開く
                            </label>
                            <input
                                id="file-reopen"
                                type="file"
                                accept=".cr2,.cr3,.nef,.nrw,.arw,.srf,.raf,.orf,.rw2,.dng,.pef,.raw"
                                onChange={(e) => {
                                    const f = e.target.files?.[0];
                                    if (f) handleFileSelected(f);
                                }}
                                style={{ display: "none" }}
                            />
                        </div>
                    )}

                    {status === "error" && (
                        <div className="editor__error">
                            <p>エラーが発生しました</p>
                            <p className="editor__error-detail">{error}</p>
                        </div>
                    )}
                </div>

                {/* Controls panel */}
                {(status === "ready" || status === "processing") && (
                    <aside className="editor__sidebar">
                        <Controls
                            params={params}
                            onChange={handleParamsChange}
                            onExport={handleExport}
                            onReset={handleReset}
                            disabled={status !== "ready"}
                        />
                        <Histogram
                            imageData={histogramData}
                            width={metadata?.width ?? 0}
                            height={metadata?.height ?? 0}
                        />
                    </aside>
                )}
            </div>
        </div>
    );
}
