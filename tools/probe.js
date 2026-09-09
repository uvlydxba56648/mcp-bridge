const { Client } = require('@modelcontextprotocol/sdk/client/index.js');
const { StreamableHTTPClientTransport } = require('@modelcontextprotocol/sdk/client/streamableHttp.js');
const { SSEClientTransport } = require('@modelcontextprotocol/sdk/client/sse.js');

async function tryStream() {
  const client = new Client({ name: 'mcp-bridge-probe', version: '0.1.0' });
  const transport = new StreamableHTTPClientTransport(new URL('http://127.0.0.1:64342/stream'));
  await client.connect(transport);
  return client;
}
async function trySSE() {
  const client = new Client({ name: 'mcp-bridge-probe', version: '0.1.0' });
  const transport = new SSEClientTransport(new URL('http://127.0.0.1:64342/sse'));
  await client.connect(transport);
  return client;
}

(async () => {
  let client, used;
  try { client = await tryStream(); used = 'streamable-http'; }
  catch (e) { console.error('stream failed:', e.message); client = await trySSE(); used = 'sse'; }

  const serverInfo = client.getServerVersion();
  const caps = client.getServerCapabilities();
  console.log('TRANSPORT:', used);
  console.log('SERVER:', JSON.stringify(serverInfo));
  console.log('CAPABILITIES:', JSON.stringify(caps));

  const tools = await client.listTools();
  console.log('TOOL_COUNT:', tools.tools.length);
  const out = {
    transport: used,
    server: serverInfo,
    capabilities: caps,
    tools: tools.tools.map(t => ({ name: t.name, description: t.description, inputSchema: t.inputSchema, annotations: t.annotations }))
  };
  require('fs').writeFileSync('/home/qy/workSpace/JetBrainsmcp/docs/jetbrains-mcp-tools.json', JSON.stringify(out, null, 2));
  for (const t of tools.tools) console.log('-', t.name);
  await client.close();
  process.exit(0);
})().catch(e => { console.error('FATAL', e); process.exit(1); });
