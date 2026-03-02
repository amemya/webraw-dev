# Web RAW Processor — 技術仕様書

ブラウザ上で RAW 画像ファイル（CR2 等）を読み込み、現像パラメータをリアルタイムに調整し、JPEG としてエクスポートできる Web アプリケーション。

## アーキテクチャ概要

```mermaid
graph LR
    subgraph Browser
        A[File Input] -->|ArrayBuffer| B[WASM Module<br/>Rust + rawloader]
        B -->|Linear sRGB f32<br/>(DCP Color Pipeline)<br/>+ Metadata| C[WebGPU Pipeline<br/>Compute Shader]
        C -->|sRGB u8| D[Canvas Preview]
        E[UI Controls<br/>Next.js + TS] -->|Parameters| C
        D --> F[JPEG Export]
    end
```

### データフロー

```
1. ユーザーが RAW ファイルをドロップ
2. File API → ArrayBuffer としてメモリに読み込み
3. ArrayBuffer を WASM (Rust) に渡す
4. rawloader::decode() で RAW デコード
5. Black/White level 補正 + カメラWB → Bayer デモザイク → Highlight Desaturation
6. DCPプロファイルによる DNG Color Pipeline 適用 (CamRGB → ProPhoto → HSV LUT → ToneCurve → Linear sRGB) またはカラーマトリクスによる補正
7. デコード結果 (Linear sRGB f32) とメタデータ (WB係数, CFA, カラーマトリクス等) を JS 側に返却
8. Pixelデータを WebGPU Storage Buffer にアップロード
9. Compute Shader で画像処理:
   (ユーザー追加WB) → 露出 → ハイライト保護 → コントラスト → ハイライト/シャドウ → 彩度 → sRGB ガンマ
10. 処理済みデータを Canvas にレンダリング
11. スライダー操作 → パラメータ変更 → ステップ 9 のみ再実行（リアルタイム）
```

> **Note:** ステップ 4〜7 の RAW デコードと基本カラー変換は **初回のみ** 実行。以降はステップ 9 の GPU パイプラインだけが再実行されるため、スライダー操作はリアルタイムに反映される。

---

## 技術スタック

| レイヤー | 技術 | 役割 |
|---------|------|------|
| フロントエンド | **Next.js 16 + TypeScript** | UI、状態管理 |
| RAW デコード | **Rust + rawloader 0.37 + wasm-bindgen** | RAW → Linear RGB 変換 |
| 画像処理 | **WebGPU Compute Shaders (WGSL)** | GPU 並列画像処理 |
| ビルド | **wasm-pack** (`--target web`) | Rust → WASM コンパイル |
| 型定義 | **@webgpu/types** | WebGPU API の TypeScript 型 |

---

## プロジェクト構成

```
web-raw-dev/
├── app/
│   ├── layout.tsx                # ルートレイアウト
│   ├── page.tsx                  # メインページ (dynamic import, SSR無効)
│   └── globals.css               # グローバルスタイル
├── components/
│   ├── Editor.tsx                # メインエディタ (状態管理 + ワークフロー統合)
│   ├── Controls.tsx              # 調整スライダー群 + 色温度→WB変換
│   ├── FileDropZone.tsx          # ドラッグ&ドロップ + ファイル選択
│   └── Histogram.tsx             # RGBL ヒストグラム表示
├── lib/
│   ├── types.ts                  # 共通型定義 (ProcessingParams, DecodedImage 等)
│   ├── wasm/
│   │   └── index.ts              # WASM ロード + decodeRaw() 公開
│   └── gpu/
│       ├── device.ts             # WebGPU デバイス初期化 + 検出
│       ├── pipeline.ts           # ImageProcessor クラス (バッファ管理, dispatch)
│       └── shaders/
│           └── process.wgsl      # 統合 Compute Shader
├── crate/                        # Rust WASM クレート
│   ├── Cargo.toml
│   ├── src/
│   │   ├── lib.rs                # WASM エントリ: decode_raw(&[u8]) → JsValue
│   │   ├── decode.rs             # rawloader ラッパー + Black/White level 正規化
│   │   └── demosaic.rs           # Bayer bilinear デモザイク
│   └── pkg/                      # wasm-pack ビルド出力 (gitignore対象)
├── types/
│   └── wgsl.d.ts                 # WGSL モジュール宣言
├── next.config.ts                # webpack: WGSL loader + async WASM
├── tsconfig.json
└── package.json
```

---

## コンポーネント詳細

### 1. Rust WASM クレート (`crate/`)

#### 公開 API

```rust
#[wasm_bindgen]
pub fn decode_raw(data: &[u8]) -> Result<JsValue, JsError>
// 返り値: { pixels: Float32Array, width, height, metadata: { wb_coeffs, color_matrix, ... }, display_referred: bool }
```

#### 処理内容

- `rawloader::decode(&mut Cursor<&[u8]>)` で RAW デコード
- `RawImage` から CFA パターン、WB 係数、Black/White レベルを抽出
- Black level 減算 + White level 正規化と、プレデモザイクWB適用 → f32 [0.0, 1.0]
- Bayer bilinear デモザイク → RGB インターリーブ f32
- Highlight Desaturation (クリップされたハイライト領域のみ彩度を落としマゼンタ被りを防止)
- DNG Color Pipeline (DCPプロファイル) 適用、またはフォールバック行列適用による Linear sRGB 化
- `serde-wasm-bindgen` で JS オブジェクトに変換

#### デモザイクとカラーパイプライン

- **デモザイク:** Bilinear 補間（将来的にAHD / VNG等に拡張予定）
- **DNG Color Pipeline:** 
  DCPプロファイルが存在する場合、以下を適用して正確な色再現とトーンカーブ処理を実施。
  CamRGB → (ForwardMatrix) → ProPhoto RGB → (HueSatMap / LookTable) → (ToneCurve) → Linear sRGB
- 対応 CFA: RGGB, BGGR, GRBG, GBRG（rawloader::CFA から自動判定）

---

### 2. WebGPU パイプライン (`lib/gpu/`)

#### ImageProcessor クラス

```typescript
class ImageProcessor {
  async init(): Promise<void>;
  async uploadImage(pixels: Float32Array, width: number, height: number): Promise<void>;
  async process(params: ProcessingParams): Promise<void>;
  async render(canvas: HTMLCanvasElement): Promise<void>;
  async readback(): Promise<Uint8ClampedArray>;
  destroy(): void;
}
```

#### Compute Shader (`process.wgsl`)

1 回の dispatch で全処理を実行する統合シェーダ:

| 処理 | 内容 |
|------|------|
| White Balance | RGB チャネルごとの乗算（ユーザー調整分のみ） |
| 露出 | `pow(2, EV)` による乗算 |
| ハイライト保護 | 露出適用後、RGBの最大値が1.0を超える場合にスケーリングで色相を保持し保護 |
| コントラスト | 0.5 中心の S カーブ |
| ハイライト/シャドウ | smoothstep マスクによる選択的調整 |
| 彩度 | Rec.709 輝度ベースの彩度調整 |
| ガンマ | sRGB ガンマエンコーディング |

Workgroup サイズ: `@workgroup_size(16, 16)`

#### Uniform バッファレイアウト (48 bytes)

```
offset 0:  width      (u32)
offset 4:  height     (u32)
offset 8:  wb_r       (f32)
offset 12: wb_g       (f32)
offset 16: wb_b       (f32)
offset 20: exposure   (f32)   // EV stops
offset 24: contrast   (f32)   // -1.0 ~ 1.0
offset 28: highlights (f32)   // -1.0 ~ 1.0
offset 32: shadows    (f32)   // -1.0 ~ 1.0
offset 36: saturation (f32)   // -1.0 ~ 1.0
offset 40: _padding   (f32)
offset 44: _padding   (f32)
```

---

### 3. フロントエンド UI

#### レイアウト

```
┌────────────────────────────────────────┐
│  ◈ RAW Processor  │ Camera  │ WxH     │
├──────────────────────┬─────────────────┤
│                      │  現像パラメータ   │
│   Canvas             │  ├ 露出          │
│   (WebGPU Preview)   │  ├ コントラスト   │
│                      │  ├ ハイライト     │
│                      │  ├ シャドウ       │
│                      │  ├ 色温度        │
│                      │  ├ 色かぶり補正   │
│                      │  └ 彩度          │
├──────────────────────┤                 │
│   Histogram (RGBL)   │  [JPEG Export]  │
└──────────────────────┴─────────────────┘
```

#### 調整パラメータ

| パラメータ | 範囲 | ステップ | デフォルト |
|-----------|------|---------|----------|
| 露出 | -5 ~ +5 EV | 0.1 | 0 |
| コントラスト | -100 ~ +100 | 1 | 0 |
| ハイライト | -100 ~ +100 | 1 | 0 |
| シャドウ | -100 ~ +100 | 1 | 0 |
| 色温度 | 2000 ~ 12000 K | 100 | 6500 |
| 色かぶり補正 | -150 ~ +150 | 1 | 0 |
| 彩度 | -100 ~ +100 | 1 | 0 |

#### WASM ロード戦略

`next/dynamic` で SSR を無効化し、クライアントサイドでのみ WASM モジュールを遅延ロード。

---

## 対応 RAW フォーマット

rawloader 0.37 がサポートする全フォーマット。主要なもの:

| フォーマット | メーカー |
|------------|---------|
| CR2 / CR3 | Canon |
| NEF / NRW | Nikon |
| ARW / SRF | Sony |
| RAF | Fujifilm |
| ORF | Olympus |
| RW2 | Panasonic |
| DNG | Adobe (汎用) |
| PEF | Pentax |

---

## WebGPU 対応状況 (2026年3月時点)

| ブラウザ | 対応状況 |
|---------|---------|
| Chrome / Edge | ✅ 安定版で利用可能 |
| Firefox | ⚠️ 限定的サポート |
| Safari | ⚠️ 実験的 |

Phase 1 では WebGPU 必須（Chrome/Edge 推奨）。非対応ブラウザには警告を表示。

---

## ビルド・実行

```bash
# WASM ビルド (初回 or Rust 変更時)
npm run wasm:build

# 開発サーバー起動
npm run dev

# プロダクションビルド
npm run build
```

---

## 今後の拡張 (Phase 2+)

- WebGPU 非対応ブラウザ向け CPU フォールバック (Rust WASM 内処理)
- 高品質デモザイク (AHD / VNG)
- ユーザーカスタムLUT / カラーグレーディングのインポート
- ノイズリダクション (Compute Shader)
- シャープニング
- PON エクスポート等の他フォーマット対応
- WASM SIMD 最適化
