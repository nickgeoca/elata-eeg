/** @type {import('next').NextConfig} */
const nextConfig = {
  // Note: WebSocket proxying is handled by connecting directly to the daemon.
  async rewrites() {
    return [
      {
        source: "/api/:path*",
        destination: "http://127.0.0.1:9000/api/:path*",
      },
    ];
  },
};

module.exports = nextConfig;