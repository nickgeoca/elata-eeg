import type { NextConfig } from "next";
import path from 'path'; // Import the 'path' module

const nextConfig: NextConfig = {
  // Redirect /api requests to the daemon
  async rewrites() {
    const daemonHost = process.env.DAEMON_HOST || '127.0.0.1';
    return [
      {
        source: '/api/:path*',
        destination: `http://${daemonHost}:9000/api/:path*`,
      },
    ];
  },

  webpack: (config, { buildId, dev, isServer, defaultLoaders, webpack }) => {
    // Add aliases for dependencies used by applet files outside the kiosk directory
    // This tells Webpack to resolve these modules to the specific paths
    // when imported from files outside the 'kiosk' directory structure.
    if (!config.resolve) {
      config.resolve = {};
    }
    if (!config.resolve.alias) {
      config.resolve.alias = {};
    }
    if (!config.resolve.modules) {
      config.resolve.modules = [];
    }
    
    // Add module resolution paths
    config.resolve.modules.push(path.resolve(__dirname, 'node_modules'));
    config.resolve.modules.push(path.resolve(__dirname, '../applets'));
    
    // Add aliases for dependencies used by applet files
    // @ts-ignore because alias can be an object or array
    config.resolve.alias['webgl-plot'] = path.resolve(__dirname, 'node_modules/webgl-plot');
    
    // Important: return the modified config
    return config;
  },
};

export default nextConfig;
