const { createServer } = require('http');
const { parse } = require('url');
const next = require('next');
const httpProxy = require('http-proxy');

const dev = process.env.NODE_ENV !== 'production';
const app = next({ dev });
const handle = app.getRequestHandler();

const proxy = httpProxy.createProxyServer({
  timeout: 60000, // 60-second timeout for the proxy
});

proxy.on('error', (err, req, res) => {
  console.error('Proxy error:', err);
  if (res.writeHead) {
    res.writeHead(500, {
      'Content-Type': 'text/plain',
    });
  }
  res.end('Something went wrong. And we are reporting a custom error message.');
});

app.prepare().then(() => {
  const server = createServer((req, res) => {
    const parsedUrl = parse(req.url, true);
    const { pathname } = parsedUrl;

    if (pathname.startsWith('/api/')) {
      proxy.web(req, res, { target: 'http://raspberrypi.local:9000' });
    } else {
      handle(req, res, parsedUrl);
    }
  });

  server.on('upgrade', (req, socket, head) => {
    const parsedUrl = parse(req.url, true);
    const { pathname } = parsedUrl;

    if (pathname.startsWith('/ws/data')) {
      console.log('Proxying WebSocket request to /ws/data');
      proxy.ws(req, socket, head, { target: 'ws://raspberrypi.local:9001' });
    } else {
      socket.destroy();
    }
  });

  server.listen(3000, (err) => {
    if (err) throw err;
    console.log('> Ready on http://localhost:3000');
  });
});