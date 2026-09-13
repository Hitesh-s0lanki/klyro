"use strict";

// Always receive buffers: decoding a float32 vector as UTF-8 destroys its bytes.
const text = value => Buffer.isBuffer(value) ? value.toString('utf8') : String(value);
function pairs(value) {
    if (!Array.isArray(value) || value.length % 2) throw new TypeError('Invalid memory map reply');
    return Array.from({ length: value.length / 2 }, (_, i) => [value[i * 2], value[i * 2 + 1]]);
}
const object = value => Object.fromEntries(pairs(value).map(([key, val]) => [text(key), val]));
function vector(value) {
    if (Buffer.isBuffer(value)) return value;
    const bytes = Buffer.alloc(value.length * 4);
    for (let i = 0; i < value.length; i++) bytes.writeFloatLE(value[i], i * 4);
    return bytes;
}
function decodeVector(bytes) {
    if (bytes === null) return null;
    if (!Buffer.isBuffer(bytes) || bytes.length % 4) throw new TypeError('Invalid vector reply');
    return Float32Array.from({ length: bytes.length / 4 }, (_, i) => bytes.readFloatLE(i * 4));
}
function weights(args, value) {
    if (value !== undefined) args.push('WEIGHTS', value.keyword, value.vector, value.recency, value.importance);
}
function filters(args, clauses = []) {
    for (const { field, op, value } of clauses) args.push('FILTER', field, op, value);
}
function flags(args, options) {
    for (const [key, flag] of [['noText', 'NOTEXT'], ['withMeta', 'WITHMETA'], ['withVector', 'WITHVEC'], ['withScores', 'WITHSCORES']]) {
        if (options[key]) args.push(flag);
    }
}
function retrieval(args, options) {
    if (options.topK !== undefined) args.push('TOPK', options.topK);
    filters(args, options.filters);
    flags(args, options);
}
function entries(meta) { return meta instanceof Map ? [...meta] : Object.entries(meta); }
function nonempty(values) {
    if (!values.length) throw new TypeError('At least one item is required');
    return values;
}
exports.memoryApi = function memoryApi(client, binary = false) {
    const decode = value => binary ? value : text(value);
    const call = (command, ...args) => client.callBuffer(`MEM.${command}`, ...args);
    function record(reply) {
        if (reply === null) return null;
        const result = object(reply);
        for (const key of ['id', 'text']) if (key in result) result[key] = decode(result[key]);
        for (const key of ['importance', 'created_at', 'updated_at', 'pttl', 'score', 'keyword_score', 'vector_score', 'recency_score']) {
            if (key in result) result[key] = Number(result[key]);
        }
        if ('meta' in result) result.meta = new Map(pairs(result.meta).map(([key, value]) => [decode(key), decode(value)]));
        if ('vector' in result) result.vector = decodeVector(result.vector);
        return result;
    }
    return {
        async create(key, options) {
            const args = [key];
            for (const [field, token] of [['mode', 'MODE'], ['dim', 'DIM'], ['metric', 'METRIC'], ['halflife', 'HALFLIFE']]) {
                if (options[field] !== undefined) args.push(token, options[field]);
            }
            weights(args, options.weights);
            return text(await call('CREATE', ...args));
        },
        async info(key) {
            const result = object(await call('INFO', key));
            result.mode = text(result.mode);
            result.metric = text(result.metric);
            result.weights = Object.fromEntries(pairs(result.weights).map(([key, value]) => [text(key), Number(value)]));
            for (const key of ['dim', 'halflife', 'records', 'vectors', 'terms', 'avg_doc_len', 'bytes']) result[key] = Number(result[key]);
            return result;
        },
        async config(key, options) {
            const args = [key];
            weights(args, options.weights);
            if (options.halflife !== undefined) args.push('HALFLIFE', options.halflife);
            return text(await call('CONFIG', ...args));
        },
        async card(key) { return Number(await call('CARD', key)); },
        async add(key, options) {
            const args = [key, 'TEXT', options.text];
            for (const [field, token] of [['id', 'ID'], ['importance', 'IMPORTANCE'], ['ttl', 'TTL']]) {
                if (options[field] !== undefined) args.push(token, options[field]);
            }
            if (options.vector !== undefined) args.push('VEC', vector(options.vector));
            if (options.meta !== undefined) for (const [key, value] of entries(options.meta)) args.push('META', key, value);
            if (options.condition !== undefined) args.push(options.condition);
            return decode(await call('ADD', ...args));
        },
        async get(key, id, options = {}) {
            const args = [key, id]; flags(args, options);
            return record(await call('GET', ...args));
        },
        async mget(key, ...ids) { return (await call('MGET', key, ...nonempty(ids))).map(record); },
        async del(key, ...ids) { return Number(await call('DEL', key, ...nonempty(ids))); },
        async setMeta(key, id, meta) { return Number(await call('SETMETA', key, id, ...nonempty(entries(meta)).flat())); },
        async delMeta(key, id, ...fields) { return Number(await call('DELMETA', key, id, ...nonempty(fields))); },
        async expire(key, id, seconds) { return Number(await call('EXPIRE', key, id, seconds)) === 1; },
        async scan(key, cursor, options = {}) {
            const args = [key, cursor];
            if (options.count !== undefined) args.push('COUNT', options.count);
            filters(args, options.filters);
            const [next, ids] = await call('SCAN', ...args);
            return { cursor: text(next), ids: ids.map(decode) };
        },
        async search(key, query, options = {}) {
            const args = [key, query]; retrieval(args, options);
            return (await call('SEARCH', ...args)).map(record);
        },
        async vsearch(key, embedding, options = {}) {
            const args = [key, 'VEC', vector(embedding)]; retrieval(args, options);
            return (await call('VSEARCH', ...args)).map(record);
        },
        async query(key, options) {
            const args = [key];
            if (options.text !== undefined) args.push('TEXT', options.text);
            if (options.vector !== undefined) args.push('VEC', vector(options.vector));
            if (options.fusion !== undefined) args.push('FUSION', options.fusion);
            weights(args, options.weights); retrieval(args, options);
            return (await call('QUERY', ...args)).map(record);
        },
    };
};
