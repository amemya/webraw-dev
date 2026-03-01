"use client";

import React, { useCallback, useState } from "react";

interface FileDropZoneProps {
    onFileSelected: (file: File) => void;
    isLoading: boolean;
}

export default function FileDropZone({ onFileSelected, isLoading }: FileDropZoneProps) {
    const [isDragOver, setIsDragOver] = useState(false);

    const handleDragOver = useCallback((e: React.DragEvent) => {
        e.preventDefault();
        e.stopPropagation();
        setIsDragOver(true);
    }, []);

    const handleDragLeave = useCallback((e: React.DragEvent) => {
        e.preventDefault();
        e.stopPropagation();
        setIsDragOver(false);
    }, []);

    const handleDrop = useCallback(
        (e: React.DragEvent) => {
            e.preventDefault();
            e.stopPropagation();
            setIsDragOver(false);

            const files = e.dataTransfer.files;
            if (files.length > 0) {
                onFileSelected(files[0]);
            }
        },
        [onFileSelected]
    );

    const handleFileInput = useCallback(
        (e: React.ChangeEvent<HTMLInputElement>) => {
            const files = e.target.files;
            if (files && files.length > 0) {
                onFileSelected(files[0]);
            }
        },
        [onFileSelected]
    );

    return (
        <div
            className={`dropzone ${isDragOver ? "dropzone--active" : ""} ${isLoading ? "dropzone--loading" : ""}`}
            onDragOver={handleDragOver}
            onDragLeave={handleDragLeave}
            onDrop={handleDrop}
        >
            {isLoading ? (
                <div className="dropzone__loading">
                    <div className="dropzone__spinner" />
                    <p>RAW ファイルをデコード中...</p>
                </div>
            ) : (
                <>
                    <div className="dropzone__icon">
                        <svg width="64" height="64" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
                            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                            <polyline points="17,8 12,3 7,8" />
                            <line x1="12" y1="3" x2="12" y2="15" />
                        </svg>
                    </div>
                    <p className="dropzone__text">RAW ファイルをドラッグ＆ドロップ</p>
                    <p className="dropzone__subtext">CR2, NEF, ARW, DNG, RAF, ORF, RW2, PEF</p>
                    <label className="dropzone__button" htmlFor="file-input">
                        ファイルを選択
                    </label>
                    <input
                        id="file-input"
                        type="file"
                        accept=".cr2,.cr3,.nef,.nrw,.arw,.srf,.raf,.orf,.rw2,.dng,.pef,.raw"
                        onChange={handleFileInput}
                        style={{ display: "none" }}
                    />
                </>
            )}
        </div>
    );
}
