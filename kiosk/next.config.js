/** @type {import('next').NextConfig} */
const nextConfig = {
  async rewrites() {
    return [
      {
        source: '/api/:path*',
        destination: 'http://127.0.0.1:9000/api/:path*',
      },
      {
        source: '/ws',
        destination: 'http://127.0.0.1:9000/ws',
      },
    ]
  },
};

module.exports = nextConfig;