"use client";

import React, { useRef, useEffect } from "react";

interface HistogramProps {
    imageData: Uint8ClampedArray | null;
    width: number;
    height: number;
}

export default function Histogram({ imageData, width, height }: HistogramProps) {
    const canvasRef = useRef<HTMLCanvasElement>(null);

    useEffect(() => {
        if (!imageData || !canvasRef.current) return;

        const canvas = canvasRef.current;
        const ctx = canvas.getContext("2d");
        if (!ctx) return;

        const cw = canvas.width;
        const ch = canvas.height;

        // Calculate histogram bins
        const rBins = new Uint32Array(256);
        const gBins = new Uint32Array(256);
        const bBins = new Uint32Array(256);
        const lBins = new Uint32Array(256);

        for (let i = 0; i < width * height; i++) {
            const idx = i * 4;
            const r = imageData[idx];
            const g = imageData[idx + 1];
            const b = imageData[idx + 2];
            const l = Math.round(0.2126 * r + 0.7152 * g + 0.0722 * b);

            rBins[r]++;
            gBins[g]++;
            bBins[b]++;
            lBins[l]++;
        }

        // Find max for normalization
        let max = 0;
        for (let i = 0; i < 256; i++) {
            max = Math.max(max, rBins[i], gBins[i], bBins[i], lBins[i]);
        }

        // Use log scale for better visualization
        const logMax = Math.log(max + 1);

        // Clear
        ctx.clearRect(0, 0, cw, ch);

        // Draw background
        ctx.fillStyle = "rgba(0, 0, 0, 0.3)";
        ctx.fillRect(0, 0, cw, ch);

        // Draw histograms with transparency
        const drawChannel = (bins: Uint32Array, color: string) => {
            ctx.beginPath();
            ctx.moveTo(0, ch);
            for (let i = 0; i < 256; i++) {
                const x = (i / 255) * cw;
                const h = (Math.log(bins[i] + 1) / logMax) * ch;
                ctx.lineTo(x, ch - h);
            }
            ctx.lineTo(cw, ch);
            ctx.closePath();
            ctx.fillStyle = color;
            ctx.fill();
        };

        // Draw in order: luminance (back), then RGB channels
        drawChannel(lBins, "rgba(180, 180, 180, 0.25)");
        drawChannel(rBins, "rgba(255, 80, 80, 0.35)");
        drawChannel(gBins, "rgba(80, 200, 80, 0.35)");
        drawChannel(bBins, "rgba(80, 120, 255, 0.35)");
    }, [imageData, width, height]);

    return (
        <div className="histogram">
            <canvas
                ref={canvasRef}
                width={256}
                height={100}
                className="histogram__canvas"
            />
        </div>
    );
}
