// lib/gpu/pipeline.ts
// WebGPU compute pipeline for real-time image processing

import type { ProcessingParams } from "../types";
import processShaderSource from "./shaders/process.wgsl";

export class ImageProcessor {
    private device: GPUDevice;

    private pipeline: GPUComputePipeline | null = null;
    private inputBuffer: GPUBuffer | null = null;
    private outputBuffer: GPUBuffer | null = null;
    private paramsBuffer: GPUBuffer | null = null;
    private toneCurveBuffer: GPUBuffer | null = null;
    private bindGroup: GPUBindGroup | null = null;

    private imageWidth = 0;
    private imageHeight = 0;
    private lastRenderedData: Float32Array | null = null;
    private processing = false;

    constructor(device: GPUDevice) {
        this.device = device;
    }

    async init(): Promise<void> {
        const shaderModule = this.device.createShaderModule({
            label: "process-shader",
            code: processShaderSource,
        });

        const bindGroupLayout = this.device.createBindGroupLayout({
            label: "process-bind-group-layout",
            entries: [
                {
                    binding: 0,
                    visibility: GPUShaderStage.COMPUTE,
                    buffer: { type: "read-only-storage" },
                },
                {
                    binding: 1,
                    visibility: GPUShaderStage.COMPUTE,
                    buffer: { type: "storage" },
                },
                {
                    binding: 2,
                    visibility: GPUShaderStage.COMPUTE,
                    buffer: { type: "uniform" },
                },
                {
                    binding: 3,
                    visibility: GPUShaderStage.COMPUTE,
                    buffer: { type: "read-only-storage" },
                },
            ],
        });

        this.pipeline = this.device.createComputePipeline({
            label: "process-pipeline",
            layout: this.device.createPipelineLayout({
                bindGroupLayouts: [bindGroupLayout],
            }),
            compute: {
                module: shaderModule,
                entryPoint: "main",
            },
        });

        // Create uniform buffer for params (12 floats = 48 bytes, aligned to 16)
        this.paramsBuffer = this.device.createBuffer({
            label: "params-buffer",
            size: 48, // 12 * 4 bytes
            usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST,
        });
    }

    async uploadImage(
        pixels: Float32Array,
        width: number,
        height: number
    ): Promise<void> {
        this.imageWidth = width;
        this.imageHeight = height;

        const pixelCount = width * height * 3;
        const byteSize = pixelCount * 4;

        // Create input buffer with the pixel data
        this.inputBuffer = this.device.createBuffer({
            label: "input-pixels",
            size: byteSize,
            usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST,
        });
        this.device.queue.writeBuffer(this.inputBuffer, 0, pixels);

        // Create output buffer
        this.outputBuffer = this.device.createBuffer({
            label: "output-pixels",
            size: byteSize,
            usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_SRC,
        });

        // Create default empty tone curve buffer if metadata not loaded yet
        if (!this.toneCurveBuffer) {
            this.toneCurveBuffer = this.device.createBuffer({
                label: "tonecurve-buffer",
                size: 4, // minimum size
                usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST,
            });
            const defaultCurve = new Float32Array([0.0]);
            this.device.queue.writeBuffer(this.toneCurveBuffer, 0, defaultCurve);
        }

        this._createBindGroup();
    }

    async uploadMetadata(metadata: import("../types").RawMetadata): Promise<void> {
        if (metadata.tone_curve && metadata.tone_curve.length > 0) {
            const curveData = new Float32Array(metadata.tone_curve);
            if (this.toneCurveBuffer) this.toneCurveBuffer.destroy();

            this.toneCurveBuffer = this.device.createBuffer({
                label: "tonecurve-buffer",
                size: Math.max(curveData.byteLength, 4),
                usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST,
            });
            this.device.queue.writeBuffer(this.toneCurveBuffer, 0, curveData);

            if (this.inputBuffer && this.outputBuffer && this.paramsBuffer) {
                this._createBindGroup();
            }
        }
    }

    private _createBindGroup() {
        if (!this.pipeline || !this.inputBuffer || !this.outputBuffer || !this.paramsBuffer || !this.toneCurveBuffer) return;

        // Create bind group
        this.bindGroup = this.device.createBindGroup({
            label: "process-bind-group",
            layout: this.pipeline.getBindGroupLayout(0),
            entries: [
                { binding: 0, resource: { buffer: this.inputBuffer } },
                { binding: 1, resource: { buffer: this.outputBuffer } },
                { binding: 2, resource: { buffer: this.paramsBuffer } },
                { binding: 3, resource: { buffer: this.toneCurveBuffer } },
            ],
        });
    }

    async process(params: ProcessingParams): Promise<void> {
        if (!this.pipeline || !this.bindGroup || !this.paramsBuffer || !this.outputBuffer) {
            throw new Error("Pipeline not initialized or no image uploaded");
        }

        // Prevent concurrent processing — skip if already processing
        if (this.processing) {
            return;
        }
        this.processing = true;

        try {
            // Pack params into uniform buffer
            const paramsData = new ArrayBuffer(48);
            const u32View = new Uint32Array(paramsData, 0, 2);
            const f32View = new Float32Array(paramsData, 8, 10);

            u32View[0] = this.imageWidth;
            u32View[1] = this.imageHeight;
            f32View[0] = params.whiteBalance[0]; // wb_r
            f32View[1] = params.whiteBalance[1]; // wb_g
            f32View[2] = params.whiteBalance[2]; // wb_b
            f32View[3] = params.exposure;
            f32View[4] = params.contrast / 100.0; // normalize to -1..1
            f32View[5] = params.highlights / 100.0;
            f32View[6] = params.shadows / 100.0;
            f32View[7] = params.saturation / 100.0;
            f32View[8] = 0; // padding
            f32View[9] = 0; // padding

            this.device.queue.writeBuffer(this.paramsBuffer, 0, paramsData);

            // Dispatch compute shader
            const encoder = this.device.createCommandEncoder();
            const pass = encoder.beginComputePass();
            pass.setPipeline(this.pipeline);
            pass.setBindGroup(0, this.bindGroup);

            const workgroupsX = Math.ceil(this.imageWidth / 16);
            const workgroupsY = Math.ceil(this.imageHeight / 16);
            pass.dispatchWorkgroups(workgroupsX, workgroupsY);
            pass.end();

            // Create a fresh readback buffer each time to avoid mapAsync conflicts
            const byteSize = this.imageWidth * this.imageHeight * 3 * 4;
            const readbackBuffer = this.device.createBuffer({
                label: "readback-buffer",
                size: byteSize,
                usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST,
            });

            encoder.copyBufferToBuffer(this.outputBuffer, 0, readbackBuffer, 0, byteSize);
            this.device.queue.submit([encoder.finish()]);

            // Wait for GPU, then read
            await readbackBuffer.mapAsync(GPUMapMode.READ);
            this.lastRenderedData = new Float32Array(readbackBuffer.getMappedRange().slice(0));
            readbackBuffer.unmap();
            readbackBuffer.destroy();
        } finally {
            this.processing = false;
        }
    }

    render(canvas: HTMLCanvasElement): void {
        if (!this.lastRenderedData) return;

        const ctx = canvas.getContext("2d");
        if (!ctx) return;

        canvas.width = this.imageWidth;
        canvas.height = this.imageHeight;

        const imageData = ctx.createImageData(this.imageWidth, this.imageHeight);
        const pixels = imageData.data;
        const data = this.lastRenderedData;

        for (let i = 0; i < this.imageWidth * this.imageHeight; i++) {
            const srcIdx = i * 3;
            const dstIdx = i * 4;
            pixels[dstIdx] = Math.round(Math.min(255, Math.max(0, data[srcIdx] * 255)));         // R
            pixels[dstIdx + 1] = Math.round(Math.min(255, Math.max(0, data[srcIdx + 1] * 255))); // G
            pixels[dstIdx + 2] = Math.round(Math.min(255, Math.max(0, data[srcIdx + 2] * 255))); // B
            pixels[dstIdx + 3] = 255;                                                              // A
        }

        ctx.putImageData(imageData, 0, 0);
    }

    readback(): Uint8ClampedArray {
        if (!this.lastRenderedData) throw new Error("No image processed");

        const data = this.lastRenderedData;
        const result = new Uint8ClampedArray(this.imageWidth * this.imageHeight * 4);
        for (let i = 0; i < this.imageWidth * this.imageHeight; i++) {
            const srcIdx = i * 3;
            const dstIdx = i * 4;
            result[dstIdx] = Math.round(Math.min(255, Math.max(0, data[srcIdx] * 255)));
            result[dstIdx + 1] = Math.round(Math.min(255, Math.max(0, data[srcIdx + 1] * 255)));
            result[dstIdx + 2] = Math.round(Math.min(255, Math.max(0, data[srcIdx + 2] * 255)));
            result[dstIdx + 3] = 255;
        }

        return result;
    }

    getImageDimensions(): { width: number; height: number } {
        return { width: this.imageWidth, height: this.imageHeight };
    }

    destroy(): void {
        this.inputBuffer?.destroy();
        this.outputBuffer?.destroy();
        this.paramsBuffer?.destroy();
        this.toneCurveBuffer?.destroy();
        this.lastRenderedData = null;
    }
}
