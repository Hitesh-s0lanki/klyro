import type { Redis, RedisOptions } from "ioredis";

export type KlyroClientOptions = RedisOptions;
export type MemoryBytes = string | Buffer;
/** Arrays are encoded as float32 little-endian; Buffer must already use that format. */
export type MemoryVector = readonly number[] | Float32Array | Buffer;
export type MemoryMode = "SEARCH" | "VECTOR" | "HYBRID";
export type MemoryMetric = "COSINE" | "L2" | "IP";
export type MemoryFusion = "LINEAR" | "RRF";
export interface MemoryWeights { keyword: number; vector: number; recency: number; importance: number; }
export type MemoryCreateOptions = {
    metric?: MemoryMetric;
    weights?: MemoryWeights;
    /** Recency half-life in seconds. */
    halflife?: number;
} & ({ mode: "SEARCH"; dim?: never } | { mode?: "VECTOR" | "HYBRID"; dim: number });
export type MemoryConfigOptions =
    | { weights: MemoryWeights; halflife?: number }
    | { weights?: MemoryWeights; halflife: number };
export interface MemoryInfo {
    mode: MemoryMode; dim: number; metric: MemoryMetric; weights: MemoryWeights;
    halflife: number; records: number; vectors: number; terms: number; avg_doc_len: number; bytes: number;
}
export type MemoryMetadata = Readonly<Record<string, MemoryBytes | number>> | ReadonlyMap<MemoryBytes, MemoryBytes | number>;
export interface MemoryAddOptions {
    text: MemoryBytes; id?: MemoryBytes; vector?: MemoryVector; meta?: MemoryMetadata;
    importance?: number;
    /** Positive seconds until this record expires. */
    ttl?: number;
    condition?: "NX" | "XX";
}
export interface MemoryReturnOptions { noText?: boolean; withMeta?: boolean; withVector?: boolean; }
export interface MemoryFilter {
    field: MemoryBytes;
    op: "EQ" | "NE" | "GT" | "GTE" | "LT" | "LTE" | "IN" | "CONTAINS";
    /** IN uses a comma-separated value, as in the server protocol. */
    value: MemoryBytes | number;
}
export interface MemorySearchOptions extends MemoryReturnOptions {
    topK?: number; filters?: readonly MemoryFilter[]; withScores?: boolean;
}
export type MemoryQueryOptions = MemorySearchOptions & {
    weights?: MemoryWeights; fusion?: MemoryFusion;
} & ({ text: MemoryBytes; vector?: MemoryVector } | { text?: MemoryBytes; vector: MemoryVector });
export interface MemoryScanOptions { count?: number; filters?: readonly MemoryFilter[]; }
export interface MemoryScanResult<T = string> { cursor: string; ids: T[]; }
export interface MemoryRecord<T = string> {
    id: T;
    /** Omitted when noText is true. */
    text?: T;
    importance: number;
    /** Unix timestamps in milliseconds. */
    created_at: number; updated_at: number;
    /** Remaining milliseconds, or -1 for a record without a deadline. */
    pttl: number;
    /** Present when withMeta is true. Map preserves arbitrary metadata field bytes. */
    meta?: Map<T, T>;
    /** Present when withVector is true; null if the record has no vector. */
    vector?: Float32Array | null;
}
export interface MemoryHit<T = string> extends MemoryRecord<T> {
    score: number;
    /** Present when withScores is true. */
    keyword_score?: number; vector_score?: number; recency_score?: number;
}
/** Every currently implemented MEM.* command, with decoded replies. */
export interface MemoryClient<T = string> {
    create(key: MemoryBytes, options: MemoryCreateOptions): Promise<"OK">;
    info(key: MemoryBytes): Promise<MemoryInfo>;
    config(key: MemoryBytes, options: MemoryConfigOptions): Promise<"OK">;
    card(key: MemoryBytes): Promise<number>;
    add(key: MemoryBytes, options: MemoryAddOptions): Promise<T>;
    get(key: MemoryBytes, id: MemoryBytes, options?: MemoryReturnOptions): Promise<MemoryRecord<T> | null>;
    mget(key: MemoryBytes, id: MemoryBytes, ...ids: MemoryBytes[]): Promise<(MemoryRecord<T> | null)[]>;
    del(key: MemoryBytes, id: MemoryBytes, ...ids: MemoryBytes[]): Promise<number>;
    setMeta(key: MemoryBytes, id: MemoryBytes, meta: MemoryMetadata): Promise<number>;
    delMeta(key: MemoryBytes, id: MemoryBytes, field: MemoryBytes, ...fields: MemoryBytes[]): Promise<number>;
    /** Seconds; zero clears the deadline. False if the record does not exist. */
    expire(key: MemoryBytes, id: MemoryBytes, seconds: number): Promise<boolean>;
    scan(key: MemoryBytes, cursor: string | number, options?: MemoryScanOptions): Promise<MemoryScanResult<T>>;
    search(key: MemoryBytes, query: MemoryBytes, options?: MemorySearchOptions): Promise<MemoryHit<T>[]>;
    vsearch(key: MemoryBytes, vector: MemoryVector, options?: MemorySearchOptions): Promise<MemoryHit<T>[]>;
    query(key: MemoryBytes, options: MemoryQueryOptions): Promise<MemoryHit<T>[]>;
}
/** Standard commands retain ioredis types; unsupported server commands still error. */
export type KlyroClient = Redis & {
    readonly memory: MemoryClient<string>;
    /** Lossless Buffer IDs, text and metadata; vectors still decode to Float32Array. */
    readonly memoryBuffer: MemoryClient<Buffer>;
};
/** Connect to 127.0.0.1:7171 by default. Does not launch the server. */
export declare function createClient(options?: KlyroClientOptions): KlyroClient;
