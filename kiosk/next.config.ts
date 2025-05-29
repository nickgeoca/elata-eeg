import type { NextConfig } from "next";
import path from 'path'; // Import the 'path' module

const nextConfig: NextConfig = {
  /* config options here */
  // Enable CORS for API routes
  async headers() {
    return [
      {
        // Apply these headers to all routes
        source: '/(.*)',
        headers: [
          {
            key: 'Access-Control-Allow-Origin',
            value: '*',
          },
          {
            key: 'Access-Control-Allow-Methods',
            value: 'GET, POST, PUT, DELETE, OPTIONS',
          },
          {
            key: 'Access-Control-Allow-Headers',
            value: 'X-Requested-With, Content-Type, Accept',
          },
        ],
      },
    ];
  },
  webpack: (config, { buildId, dev, isServer, defaultLoaders, webpack }) => {
    // Add an alias for webgl-plot
    // This tells Webpack to resolve 'webgl-plot' to the specific path
    // when imported from files outside the 'kiosk' directory structure.
    if (!config.resolve) {
      config.resolve = {};
    }
    if (!config.resolve.alias) {
      config.resolve.alias = {};
    }
    // @ts-ignore because alias can be an object or array
    config.resolve.alias['webgl-plot'] = path.resolve(__dirname, 'node_modules/webgl-plot');
    
    // Important: return the modified config
    return config;
  },
};

export default nextConfig;
