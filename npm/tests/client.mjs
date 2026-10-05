// Run after `node npm/build.mjs --wrapper`: node npm/tests/client.mjs
import assert from 'node:assert/strict';
import { execFileSync, spawn } from 'node:child_process';
import { mkdtempSync, writeFileSync, rmSync, readFileSync, copyFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve, join } from 'node:path';
import { createServer, createConnection } from 'node:net';
import { once } from 'node:events';

const work = mkdtempSync(join(tmpdir(), 'klyro-types-'));
const run = (command, args) => execFileSync(command, args, { cwd: work, stdio: 'pipe' });
let server;
try {
    const packed = JSON.parse(run('npm', ['pack', resolve('npm/dist/klyro-db'), '--json']).toString())[0];
    assert(packed.files.some(file => file.path === 'index.d.ts'));
    const platform = JSON.parse(run('npm', ['pack', resolve(`npm/dist/klyro-db-${process.platform}-${process.arch}`), '--json']).toString())[0];
    writeFileSync(join(work, 'package.json'), '{"private":true}');
    run('npm', ['install', '--ignore-scripts', '--no-audit', '--no-fund', './' + packed.filename, './' + platform.filename, 'typescript@5', '@types/node@22']);
    const types = `import { createClient, type KlyroClient, type KlyroClientOptions } from 'klyro-db';
const options: KlyroClientOptions = { port: 7171, lazyConnect: true };
const client: KlyroClient = createClient(options);
const result: Promise<string | null> = client.get('key');
client.set('key', 'value');
// @ts-expect-error ports are numbers
createClient({ port: '7171' });
// @ts-expect-error GET can return null
const wrong: Promise<number> = client.get('key');
client.disconnect();
`;
    for (const extension of ['mts', 'cts']) {
        writeFileSync(join(work, 'consumer.' + extension), types + readFileSync(new URL("./memory-types.ts", import.meta.url), "utf8").replace("KlyroClient, ", ""));
        run(process.execPath, [join(work, 'node_modules/typescript/bin/tsc'), '--strict', '--noEmit', '--module', 'NodeNext', '--moduleResolution', 'NodeNext', 'consumer.' + extension]);
    }
    const probe = createServer();
    probe.listen(0, '127.0.0.1');
    await once(probe, 'listening');
    const port = probe.address().port;
    await new Promise(resolve => probe.close(resolve));
    server = spawn(join(work, 'node_modules/.bin/klyro'), [String(port), 'test.dump'], { cwd: work, stdio: 'ignore' });
    await once(server, 'spawn');
    for (let attempt = 0; ; attempt++) {
        try {
            await new Promise((resolve, reject) => {
                const socket = createConnection({ host: '127.0.0.1', port });
                socket.once('connect', () => { socket.destroy(); resolve(); });
                socket.once('error', reject);
            });
            break;
        } catch (error) {
            if (attempt >= 50) throw error;
            await new Promise(resolve => setTimeout(resolve, 100));
        }
    }
    copyFileSync(new URL('./memory.cjs', import.meta.url), join(work, 'memory.cjs'));
    writeFileSync(join(work, 'consumer.mjs'), `
import assert from 'node:assert/strict';
import { createClient } from 'klyro-db';
import { createRequire } from 'node:module';
assert.equal(typeof createRequire(import.meta.url)('klyro-db').createClient, 'function');
const defaults = createClient({ lazyConnect: true });
assert.equal(defaults.options.port, 7171);
assert.equal(defaults.options.host, '127.0.0.1');
defaults.disconnect();
const client = createClient({ port: ${port}, lazyConnect: true, retryStrategy: () => 50, connectTimeout: 1000 });
client.on('error', () => {});
try {
    await client.connect();
    assert.equal(await client.ping(), 'PONG');
    assert.equal(await client.get('missing'), null);
    assert.equal(await client.set('typed', 'hello'), 'OK');
    assert.equal(await client.get('typed'), 'hello');
    await assert.rejects(client.call('NOT_A_COMMAND'));
    await createRequire(import.meta.url)('./memory.cjs')(client);
    await client.quit();
} finally { client.disconnect(); }
`);
    execFileSync(process.execPath, ['consumer.mjs'], { cwd: work, stdio: 'pipe', timeout: 15000 });
    console.log('PASS: packed declarations, strict ESM/CJS type checks, ESM/CJS imports, defaults, live GET/SET, all 15 memory helpers, binary round trips and errors');
} finally {
    if (server && server.exitCode === null && server.signalCode === null) {
        const exited = once(server, 'exit');
        server.kill('SIGTERM');
        await exited;
    }
    rmSync(work, { recursive: true, force: true });
}
