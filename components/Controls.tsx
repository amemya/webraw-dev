"use client";

import React, { useCallback } from "react";
import type { ProcessingParams } from "@/lib/types";

interface ControlsProps {
    params: ProcessingParams;
    onChange: (params: ProcessingParams) => void;
    onExport: () => void;
    onReset: () => void;
    disabled: boolean;
}

interface SliderConfig {
    key: keyof ProcessingParams;
    label: string;
    min: number;
    max: number;
    step: number;
    unit?: string;
    isArray?: boolean;
    arrayIndex?: number;
}

const SLIDERS: SliderConfig[] = [
    { key: "exposure", label: "露出", min: -5, max: 5, step: 0.1, unit: "EV" },
    { key: "contrast", label: "コントラスト", min: -100, max: 100, step: 1 },
    { key: "highlights", label: "ハイライト", min: -100, max: 100, step: 1 },
    { key: "shadows", label: "シャドウ", min: -100, max: 100, step: 1 },
    { key: "temperature", label: "色温度", min: 2000, max: 12000, step: 100, unit: "K" },
    { key: "tint", label: "色かぶり補正", min: -150, max: 150, step: 1 },
    { key: "saturation", label: "彩度", min: -100, max: 100, step: 1 },
];

export default function Controls({ params, onChange, onExport, onReset, disabled }: ControlsProps) {
    const handleSliderChange = useCallback(
        (key: keyof ProcessingParams, value: number) => {
            // Temperature/tint changes need to be converted to WB multipliers
            if (key === "temperature" || key === "tint") {
                const temp = key === "temperature" ? value : params.temperature;
                const tint = key === "tint" ? value : params.tint;
                const wb = temperatureToWB(temp, tint);
                onChange({
                    ...params,
                    [key]: value,
                    whiteBalance: wb,
                });
            } else {
                onChange({ ...params, [key]: value });
            }
        },
        [params, onChange]
    );

    return (
        <div className="controls">
            <div className="controls__header">
                <h2 className="controls__title">現像パラメータ</h2>
                <button className="controls__reset-btn" onClick={onReset} disabled={disabled} title="リセット">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                        <path d="M1 4v6h6" />
                        <path d="M3.51 15a9 9 0 1 0 2.13-9.36L1 10" />
                    </svg>
                </button>
            </div>

            <div className="controls__sliders">
                {SLIDERS.map((slider) => {
                    const value = params[slider.key] as number;
                    const percentage =
                        ((value - slider.min) / (slider.max - slider.min)) * 100;

                    return (
                        <div key={slider.key} className="slider-group">
                            <div className="slider-group__header">
                                <label className="slider-group__label">{slider.label}</label>
                                <span className="slider-group__value">
                                    {typeof value === "number" ? value.toFixed(slider.step < 1 ? 1 : 0) : value}
                                    {slider.unit ? ` ${slider.unit}` : ""}
                                </span>
                            </div>
                            <input
                                type="range"
                                className="slider-group__input"
                                min={slider.min}
                                max={slider.max}
                                step={slider.step}
                                value={value}
                                disabled={disabled}
                                onChange={(e) =>
                                    handleSliderChange(slider.key, parseFloat(e.target.value))
                                }
                                style={{
                                    background: `linear-gradient(to right, var(--accent) 0%, var(--accent) ${percentage}%, var(--surface-2) ${percentage}%, var(--surface-2) 100%)`,
                                }}
                            />
                        </div>
                    );
                })}
            </div>

            <div className="controls__actions">
                <button
                    className="controls__export-btn"
                    onClick={onExport}
                    disabled={disabled}
                >
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                        <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                        <polyline points="7,10 12,15 17,10" />
                        <line x1="12" y1="15" x2="12" y2="3" />
                    </svg>
                    JPEG エクスポート
                </button>
            </div>
        </div>
    );
}

/// Convert color temperature (K) and tint to RGB white balance multipliers
function temperatureToWB(temperature: number, tint: number): [number, number, number] {
    // Simplified Planckian locus approximation
    // Reference white at 6500K
    const t = temperature / 100;
    let r: number, g: number, b: number;

    if (t <= 66) {
        r = 1.0;
        g = 0.39008157876 * Math.log(t) - 0.63184144378;
        b = t <= 19 ? 0 : 0.54320678911 * Math.log(t - 10) - 1.19625408914;
    } else {
        r = 1.29293618606 * Math.pow(t - 60, -0.1332047592);
        g = 1.12989086054 * Math.pow(t - 60, -0.0755148492);
        b = 1.0;
    }

    // Normalize to reference (6500K)
    const ref_t = 65;
    const ref_r = 1.0;
    const ref_g = 0.39008157876 * Math.log(ref_t) - 0.63184144378;
    const ref_b = 0.54320678911 * Math.log(ref_t - 10) - 1.19625408914;

    r = Math.max(0.1, (ref_r / r));
    g = Math.max(0.1, (ref_g / g));
    b = Math.max(0.1, (ref_b / b));

    // Normalize so green = 1
    const scale = 1.0 / g;
    r *= scale;
    g = 1.0;
    b *= scale;

    // Apply tint (green-magenta shift)
    const tintFactor = 1.0 + tint / 300.0;
    g *= tintFactor;

    return [r, g, b];
}
