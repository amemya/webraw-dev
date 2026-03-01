import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  webpack: (config) => {
    // Handle WGSL shader files as raw strings
    config.module.rules.push({
      test: /\.wgsl$/,
      type: "asset/source",
    });

    // Handle WASM files
    config.experiments = {
      ...config.experiments,
      asyncWebAssembly: true,
      layers: true,
    };

    return config;
  },
  // Silence Turbopack warning when using --webpack flag
  turbopack: {},
};

export default nextConfig;
