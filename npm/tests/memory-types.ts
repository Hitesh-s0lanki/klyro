import type { KlyroClient, MemoryInfo, MemoryRecord, MemoryHit, MemoryScanResult } from 'klyro-db';
export async function check(client: KlyroClient) {
    const m = client.memory;
    const ok: 'OK' = await m.create('index', { mode: 'HYBRID', dim: 2 });
    const info: MemoryInfo = await m.info('index');
    await m.config('index', { halflife: 1 });
    const card: number = await m.card('index');
    const id: string = await m.add('index', { text: 'hello', vector: [1, 0], condition: 'NX' });
    const record: MemoryRecord | null = await m.get('index', id, { withMeta: true });
    const records: (MemoryRecord | null)[] = await m.mget('index', id);
    const removed: number = await m.del('index', id);
    const set: number = await m.setMeta('index', id, { field: 'value' });
    const del: number = await m.delMeta('index', id, 'field');
    const expires: boolean = await m.expire('index', id, 1);
    const scan: MemoryScanResult = await m.scan('index', '0');
    const hits: MemoryHit[] = await m.search('index', 'hello', { filters: [{ field: 'n', op: 'GTE', value: 1 }] });
    await m.vsearch('index', new Float32Array([1, 0]), { withScores: true });
    await m.query('index', { text: 'hello', vector: Buffer.alloc(8), fusion: 'RRF' });
    const binary: MemoryRecord<Buffer> | null = await client.memoryBuffer.get('index', Buffer.from('id'));
    const binaryId: Buffer = await client.memoryBuffer.add('index', { text: Buffer.alloc(1) });
    // @ts-expect-error vector mode requires dimension
    m.create('bad', { mode: 'VECTOR' });
    // @ts-expect-error search has no dimensions
    m.create('bad', { mode: 'SEARCH', dim: 2 });
    // @ts-expect-error query requires text or vector
    m.query('bad', { topK: 1 });
    // @ts-expect-error add requires text
    m.add('bad', { id: 'x' });
    // @ts-expect-error only supported filters
    m.search('bad', 'x', { filters: [{ field: 'x', op: 'LIKE', value: 'y' }] });
    // @ts-expect-error no empty config
    m.config('bad', {});
    // @ts-expect-error mget needs an id
    m.mget('bad');
    // @ts-expect-error memoryBuffer returns Buffer IDs
    const wrong: string = await client.memoryBuffer.add('index', { text: 'x' });
}
