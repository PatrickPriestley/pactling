// A real pact-js 13 consumer test. It writes the pact file that phase2_publish.sh then
// publishes with the pact-broker CLI, which is how pact-js consumers publish today.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');
const { PactV3, MatchersV3 } = require('@pact-foundation/pact');

const provider = new PactV3({
  consumer: 'Consumer',
  provider: 'Provider',
  dir: process.env.PACT_DIR || path.join(__dirname, 'pacts'),
  logLevel: 'warn',
});

test('an order can be fetched by id', async () => {
  provider
    .given('an order exists')
    .uponReceiving('a request for an order')
    .withRequest({ method: 'GET', path: '/orders/1' })
    .willRespondWith({
      status: 200,
      headers: { 'Content-Type': 'application/json' },
      body: { id: MatchersV3.integer(1), status: MatchersV3.like('open') },
    });
  await provider.executeTest(async (mockServer) => {
    const res = await fetch(`${mockServer.url}/orders/1`);
    assert.equal(res.status, 200);
    const body = await res.json();
    assert.equal(body.id, 1);
    assert.equal(body.status, 'open');
  });
});
