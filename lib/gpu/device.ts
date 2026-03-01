// lib/gpu/device.ts
// WebGPU device initialization and capability detection

export async function isWebGPUAvailable(): Promise<boolean> {
    if (!navigator.gpu) return false;
    try {
        const adapter = await navigator.gpu.requestAdapter();
        return adapter !== null;
    } catch {
        return false;
    }
}

export async function initGPUDevice(): Promise<GPUDevice | null> {
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

    device.lost.then((info: GPUDeviceLostInfo) => {
        console.error("WebGPU device lost:", info.message);
    });

    return device;
}
