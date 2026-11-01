// Webhook receiver for captures and e2e: listens on <port>, appends every request as one
// JSON line {method, path, headers, body, receivedAt} to <log.jsonl>, and answers with the
// status in the RECEIVER_STATUS env var (default 200) and a small JSON body.
//   node e2e/receiver.js <port> <log.jsonl>
const http = require('http');
const fs = require('fs');
const [port, log] = process.argv.slice(2);
if (!port || !log) {
  console.error('usage: node receiver.js <port> <log.jsonl>');
  process.exit(2);
}
const status = Number(process.env.RECEIVER_STATUS || 200);
http.createServer((req, res) => {
  let body = '';
  req.on('data', (chunk) => { body += chunk; });
  req.on('end', () => {
    fs.appendFileSync(log, JSON.stringify({
      method: req.method, path: req.url, headers: req.headers, body, receivedAt: new Date().toISOString(),
    }) + '\n');
    res.writeHead(status, { 'content-type': 'application/json' });
    res.end(JSON.stringify({ received: true }));
  });
}).listen(Number(port), '0.0.0.0', () => console.log(`receiver listening on ${port}`));
