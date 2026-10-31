// A real pact-js 13 provider verification: fetches pacts from the broker with consumer
// version selectors, verifies them against a tiny in-process provider, and publishes the
// results. Configured through the environment so the same script serves the capture
// (against the Ruby broker, through the logging proxy) and the e2e (against pactling).
//   BROKER            broker base URL (required)
//   PROVIDER_VERSION  provider version to publish results for (default 1)
//   PROVIDER_BRANCH   provider branch (default main)
//   SELECTORS         JSON array of consumer version selectors
//                     (default [{"mainBranch":true},{"deployedOrReleased":true},{"matchingBranch":true}])
//   PUBLISH           "false" to skip publishing results (default true)
//   FAIL              "true" to make the provider answer wrongly so verification fails
//   ORDER_ID          id the provider returns for /orders/1 (default 1)
const { test } = require('node:test');
const assert = require('node:assert/strict');
const http = require('node:http');
const { Verifier } = require('@pact-foundation/pact');

const broker = process.env.BROKER;
if (!broker) throw new Error('BROKER is required');

test('provider verifies the pacts the broker selects and publishes results', async () => {
  const fail = process.env.FAIL === 'true';
  const server = http.createServer((req, res) => {
    if (req.method === 'GET' && req.url === '/orders/1') {
      res.writeHead(fail ? 500 : 200, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify({ id: Number(process.env.ORDER_ID || 1), status: 'open' }));
    } else if (req.method === 'GET' && req.url === '/orders/999') {
      res.writeHead(404);
      res.end();
    } else {
      res.writeHead(404);
      res.end();
    }
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const { port } = server.address();
  try {
    const verifier = new Verifier({
      providerBaseUrl: `http://127.0.0.1:${port}`,
      provider: 'Provider',
      pactBrokerUrl: broker,
      consumerVersionSelectors: JSON.parse(
        process.env.SELECTORS || '[{"mainBranch":true},{"deployedOrReleased":true},{"matchingBranch":true}]',
      ),
      publishVerificationResult: process.env.PUBLISH !== 'false',
      providerVersion: process.env.PROVIDER_VERSION || '1',
      providerVersionBranch: process.env.PROVIDER_BRANCH || 'main',
      enablePending: true,
      stateHandlers: { 'an order exists': async () => {} },
      logLevel: process.env.PACT_LOG_LEVEL || 'warn',
    });
    const output = await verifier.verifyProvider();
    assert.ok(output !== undefined);
  } finally {
    server.close();
  }
});
