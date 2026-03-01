// lib/gpu/device.ts
// WebGPU device initialization and capability detection

export interface GPUContext {
    device: GPUDevice;
    context: GPUCanvasContext;
    format: GPUTextureFormat;
}

export async function isWebGPUAvailable(): Promise<boolean> {
    if (!navigator.gpu) return false;
    try {
        const adapter = await navigator.gpu.requestAdapter();
        return adapter !== null;
    } catch {
        return false;
    }
}

export async function initGPU(canvas: HTMLCanvasElement): Promise<GPUContext | null> {
    if (!navigator.gpu) {
        console.warn("WebGPU is not supported in this browser");
        return null;
    }

    const adapter = await navigator.gpu.requestAdapter({
        powerPreference: "high-performance",
    });

    if (!adapter) {
        console.warn("No WebGPU adapter found");
        return null;
    }

    const device = await adapter.requestDevice({
        requiredFeatures: [],
        requiredLimits: {
            maxStorageBufferBindingSize: adapter.limits.maxStorageBufferBindingSize,
            maxBufferSize: adapter.limits.maxBufferSize,
        },
    });

    device.lost.then((info) => {
        console.error("WebGPU device lost:", info.message);
    });

    const context = canvas.getContext("webgpu");
    if (!context) {
        console.warn("Failed to get WebGPU context");
        return null;
    }

    const format = navigator.gpu.getPreferredCanvasFormat();
    context.configure({
        device,
        format,
        alphaMode: "premultiplied",
    });

    return { device, context, format };
}
