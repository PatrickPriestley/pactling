// Logging reverse proxy: records every request and response that passes through it as one
// JSON object per line, so the exact traffic of a real client can be kept as a fixture.
//   node proxy.js <listen-port> <upstream-base-url> <log-file>
const http = require('node:http');
const fs = require('node:fs');
const [listenPort, upstream, logFile] = process.argv.slice(2);
const target = new URL(upstream);
const log = fs.createWriteStream(logFile, { flags: 'a' });
http.createServer((req, res) => {
  const chunks = [];
  req.on('data', (c) => chunks.push(c));
  req.on('end', () => {
    const body = Buffer.concat(chunks);
    const up = http.request(
      { host: target.hostname, port: target.port, method: req.method, path: req.url, headers: req.headers },
      (upRes) => {
        const out = [];
        upRes.on('data', (c) => out.push(c));
        upRes.on('end', () => {
          const resBody = Buffer.concat(out);
          log.write(JSON.stringify({
            request: { method: req.method, path: req.url, headers: req.headers, body: body.toString('utf8') },
            response: { status: upRes.statusCode, headers: upRes.headers, body: resBody.toString('utf8') },
          }) + '\n');
          res.writeHead(upRes.statusCode, upRes.headers);
          res.end(resBody);
        });
      },
    );
    up.on('error', (e) => { res.writeHead(502); res.end(String(e)); });
    up.end(body);
  });
}).listen(Number(listenPort), '127.0.0.1');
