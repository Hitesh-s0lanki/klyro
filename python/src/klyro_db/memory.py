from __future__ import annotations

from dataclasses import dataclass, field
from struct import pack, unpack
from typing import Literal, Mapping, Sequence, TypeAlias, cast

from redis import Redis

BytesLike: TypeAlias = str | bytes
Vector: TypeAlias = Sequence[float] | bytes
Mode: TypeAlias = Literal["SEARCH", "VECTOR", "HYBRID"]
Metric: TypeAlias = Literal["COSINE", "L2", "IP"]
Fusion: TypeAlias = Literal["LINEAR", "RRF"]
FilterOp: TypeAlias = Literal["EQ", "NE", "GT", "GTE", "LT", "LTE", "IN", "CONTAINS"]
Condition: TypeAlias = Literal["NX", "XX"]


@dataclass(frozen=True)
class Weights:
    keyword: float = 0.35
    vector: float = 0.50
    recency: float = 0.10
    importance: float = 0.05


@dataclass(frozen=True)
class Filter:
    field: BytesLike
    op: FilterOp
    value: BytesLike | int | float


@dataclass(frozen=True)
class MemoryCreate:
    mode: Mode = "HYBRID"
    dim: int | None = None
    metric: Metric = "COSINE"
    weights: Weights | None = None
    halflife: int | None = None


@dataclass(frozen=True)
class MemoryConfig:
    weights: Weights | None = None
    halflife: int | None = None


@dataclass(frozen=True)
class MemoryAdd:
    text: BytesLike
    id: BytesLike | None = None
    vector: Vector | None = None
    metadata: Mapping[BytesLike, BytesLike | int | float] = field(default_factory=dict)
    importance: float | None = None
    ttl: int | None = None
    condition: Condition | None = None


@dataclass(frozen=True)
class MemoryReturn:
    no_text: bool = False
    with_metadata: bool = False
    with_vector: bool = False


@dataclass(frozen=True)
class MemorySearch(MemoryReturn):
    top_k: int | None = None
    filters: Sequence[Filter] = ()
    with_scores: bool = False


@dataclass(frozen=True)
class MemoryQuery(MemorySearch):
    text: BytesLike | None = None
    vector: Vector | None = None
    weights: Weights | None = None
    fusion: Fusion | None = None


@dataclass(frozen=True)
class MemoryScan:
    count: int | None = None
    filters: Sequence[Filter] = ()


@dataclass(frozen=True)
class MemoryInfo:
    mode: Mode
    dim: int
    metric: Metric
    weights: Weights
    halflife: int
    records: int
    vectors: int
    terms: int
    avg_doc_len: float
    bytes: int


@dataclass(frozen=True)
class MemoryRecord:
    id: str
    text: str | None
    importance: float
    created_at: int
    updated_at: int
    pttl: int
    metadata: dict[str, str] | None = None
    vector: tuple[float, ...] | None = None


@dataclass(frozen=True)
class MemoryHit(MemoryRecord):
    score: float = 0.0
    keyword_score: float | None = None
    vector_score: float | None = None
    recency_score: float | None = None


@dataclass(frozen=True)
class MemoryScanResult:
    cursor: str
    ids: list[str]


def _text(value: object) -> str:
    return value.decode() if isinstance(value, bytes) else str(value)


def _mapping(value: object) -> dict[str, object]:
    if isinstance(value, dict):
        return {_text(k): v for k, v in value.items()}
    values = cast(Sequence[object], value)
    if len(values) % 2:
        raise TypeError("invalid memory map reply")
    return {_text(values[i]): values[i + 1] for i in range(0, len(values), 2)}


def _vector(value: Vector) -> bytes:
    if isinstance(value, bytes):
        return value
    values = [float(item) for item in value]
    return pack(f"<{len(values)}f", *values)


def _decode_vector(value: object) -> tuple[float, ...] | None:
    if value is None:
        return None
    raw = cast(bytes, value)
    if len(raw) % 4:
        raise TypeError("invalid vector reply")
    return unpack(f"<{len(raw) // 4}f", raw)


def _weights(args: list[object], value: Weights | None) -> None:
    if value is not None:
        args.extend(("WEIGHTS", value.keyword, value.vector, value.recency, value.importance))


def _filters(args: list[object], values: Sequence[Filter]) -> None:
    for item in values:
        args.extend(("FILTER", item.field, item.op, item.value))


def _returns(args: list[object], options: MemoryReturn) -> None:
    if options.no_text: args.append("NOTEXT")
    if options.with_metadata: args.append("WITHMETA")
    if options.with_vector: args.append("WITHVEC")
    if isinstance(options, MemorySearch) and options.with_scores: args.append("WITHSCORES")


def _retrieval(args: list[object], options: MemorySearch) -> None:
    if options.top_k is not None: args.extend(("TOPK", options.top_k))
    _filters(args, options.filters)
    _returns(args, options)


class MemoryClient:
    """Typed access to every currently implemented MEM.* command."""

    def __init__(self, redis: Redis) -> None:
        self._redis = redis

    def _call(self, command: str, *args: object) -> object:
        return self._redis.execute_command(f"MEM.{command}", *args)  # type: ignore[no-untyped-call]

    def _record(self, reply: object) -> MemoryRecord | MemoryHit | None:
        if reply is None:
            return None
        data = _mapping(reply)
        record_id = _text(data["id"])
        record_text = _text(data["text"]) if "text" in data else None
        importance = float(cast(float, data["importance"]))
        created_at = int(cast(int, data["created_at"]))
        updated_at = int(cast(int, data["updated_at"]))
        pttl = int(cast(int, data["pttl"]))
        metadata = {_text(k): _text(v) for k, v in _mapping(data["meta"]).items()} if "meta" in data else None
        vector = _decode_vector(data["vector"]) if "vector" in data else None
        if "score" not in data:
            return MemoryRecord(record_id, record_text, importance, created_at, updated_at, pttl, metadata, vector)
        def component(name: str) -> float | None:
            return float(cast(float, data[name])) if name in data else None
        return MemoryHit(record_id, record_text, importance, created_at, updated_at, pttl, metadata, vector, float(cast(float, data["score"])), component("keyword_score"), component("vector_score"), component("recency_score"))

    def create(self, key: BytesLike, options: MemoryCreate) -> Literal["OK"]:
        args: list[object] = [key, "MODE", options.mode]
        if options.dim is not None: args.extend(("DIM", options.dim))
        args.extend(("METRIC", options.metric)); _weights(args, options.weights)
        if options.halflife is not None: args.extend(("HALFLIFE", options.halflife))
        return cast(Literal["OK"], _text(self._call("CREATE", *args)))

    def info(self, key: BytesLike) -> MemoryInfo:
        data = _mapping(self._call("INFO", key)); w = _mapping(data["weights"])
        return MemoryInfo(cast(Mode, _text(data["mode"])), int(cast(int, data["dim"])), cast(Metric, _text(data["metric"])), Weights(*(float(cast(float, w[k])) for k in ("keyword", "vector", "recency", "importance"))), int(cast(int, data["halflife"])), int(cast(int, data["records"])), int(cast(int, data["vectors"])), int(cast(int, data["terms"])), float(cast(float, data["avg_doc_len"])), int(cast(int, data["bytes"])))

    def config(self, key: BytesLike, options: MemoryConfig) -> Literal["OK"]:
        args: list[object] = [key]; _weights(args, options.weights)
        if options.halflife is not None: args.extend(("HALFLIFE", options.halflife))
        if len(args) == 1: raise ValueError("weights or halflife is required")
        return cast(Literal["OK"], _text(self._call("CONFIG", *args)))

    def card(self, key: BytesLike) -> int: return int(cast(int, self._call("CARD", key)))

    def add(self, key: BytesLike, options: MemoryAdd) -> str:
        args: list[object] = [key, "TEXT", options.text]
        if options.id is not None: args.extend(("ID", options.id))
        if options.vector is not None: args.extend(("VEC", _vector(options.vector)))
        for name, value in options.metadata.items(): args.extend(("META", name, value))
        if options.importance is not None: args.extend(("IMPORTANCE", options.importance))
        if options.ttl is not None: args.extend(("TTL", options.ttl))
        if options.condition is not None: args.append(options.condition)
        return _text(self._call("ADD", *args))

    def get(self, key: BytesLike, id: BytesLike, options: MemoryReturn = MemoryReturn()) -> MemoryRecord | None:
        args: list[object] = [key, id]; _returns(args, options)
        result = self._record(self._call("GET", *args))
        return result if isinstance(result, MemoryRecord) else None

    def mget(self, key: BytesLike, id: BytesLike, *ids: BytesLike) -> list[MemoryRecord | None]:
        return [result if isinstance(result := self._record(item), MemoryRecord) else None for item in cast(Sequence[object], self._call("MGET", key, id, *ids))]

    def delete(self, key: BytesLike, id: BytesLike, *ids: BytesLike) -> int: return int(cast(int, self._call("DEL", key, id, *ids)))
    def set_metadata(self, key: BytesLike, id: BytesLike, metadata: Mapping[BytesLike, BytesLike | int | float]) -> int:
        if not metadata: raise ValueError("metadata must not be empty")
        args: list[object] = [key, id]
        for name, value in metadata.items(): args.extend((name, value))
        return int(cast(int, self._call("SETMETA", *args)))
    def delete_metadata(self, key: BytesLike, id: BytesLike, field: BytesLike, *fields: BytesLike) -> int: return int(cast(int, self._call("DELMETA", key, id, field, *fields)))
    def expire(self, key: BytesLike, id: BytesLike, seconds: int) -> bool: return bool(self._call("EXPIRE", key, id, seconds))

    def scan(self, key: BytesLike, cursor: str | int, options: MemoryScan = MemoryScan()) -> MemoryScanResult:
        args: list[object] = [key, cursor]
        if options.count is not None: args.extend(("COUNT", options.count))
        _filters(args, options.filters); reply = cast(Sequence[object], self._call("SCAN", *args))
        return MemoryScanResult(_text(reply[0]), [_text(item) for item in cast(Sequence[object], reply[1])])

    def search(self, key: BytesLike, query: BytesLike, options: MemorySearch = MemorySearch()) -> list[MemoryHit]:
        args: list[object] = [key, query]; _retrieval(args, options)
        return [cast(MemoryHit, self._record(item)) for item in cast(Sequence[object], self._call("SEARCH", *args))]

    def vector_search(self, key: BytesLike, vector: Vector, options: MemorySearch = MemorySearch()) -> list[MemoryHit]:
        args: list[object] = [key, "VEC", _vector(vector)]; _retrieval(args, options)
        return [cast(MemoryHit, self._record(item)) for item in cast(Sequence[object], self._call("VSEARCH", *args))]

    def query(self, key: BytesLike, options: MemoryQuery) -> list[MemoryHit]:
        if options.text is None and options.vector is None: raise ValueError("text or vector is required")
        args: list[object] = [key]
        if options.text is not None: args.extend(("TEXT", options.text))
        if options.vector is not None: args.extend(("VEC", _vector(options.vector)))
        if options.fusion is not None: args.extend(("FUSION", options.fusion))
        _weights(args, options.weights); _retrieval(args, options)
        return [cast(MemoryHit, self._record(item)) for item in cast(Sequence[object], self._call("QUERY", *args))]
