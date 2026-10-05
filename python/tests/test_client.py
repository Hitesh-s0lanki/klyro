from __future__ import annotations

import math

import pytest
from redis.exceptions import ResponseError

from klyro_db import (
    Filter, Klyro, MemoryAdd, MemoryConfig, MemoryCreate, MemoryQuery,
    MemoryReturn, MemoryScan, MemorySearch, Weights,
)


def test_all_memory_commands(klyro: Klyro) -> None:
    weights = Weights()
    assert klyro.memory.create("hybrid", MemoryCreate(dim=2, metric="L2", weights=weights, halflife=100)) == "OK"
    assert klyro.memory.config("hybrid", MemoryConfig(weights=weights, halflife=200)) == "OK"
    info = klyro.memory.info("hybrid")
    assert (info.mode, info.dim, info.metric, info.halflife) == ("HYBRID", 2, "L2", 200)
    assert info.weights == weights

    assert klyro.memory.add("hybrid", MemoryAdd(
        id="a", text="database preference", vector=[1, 0],
        metadata={"kind": "preference", "n": 2}, importance=.8, ttl=30, condition="NX",
    )) == "a"
    record = klyro.memory.get("hybrid", "a", MemoryReturn(with_metadata=True, with_vector=True))
    assert record is not None
    assert record.text == "database preference" and record.importance == pytest.approx(.8)
    assert record.metadata == {"kind": "preference", "n": "2"}
    assert record.vector == pytest.approx((1, 0)) and record.pttl > 0
    assert klyro.memory.card("hybrid") == 1
    assert klyro.memory.get("hybrid", "missing") is None
    assert klyro.memory.mget("hybrid", "a", "missing")[1] is None
    assert klyro.memory.get("hybrid", "a", MemoryReturn(no_text=True)).text is None
    assert klyro.memory.set_metadata("hybrid", "a", {"extra": "yes"}) == 1
    assert klyro.memory.delete_metadata("hybrid", "a", "extra") == 1
    assert klyro.memory.expire("hybrid", "a", 0)
    assert klyro.memory.get("hybrid", "a").pttl == -1
    assert not klyro.memory.expire("hybrid", "missing", 10)

    scan = klyro.memory.scan("hybrid", 0, MemoryScan(count=10, filters=[Filter("kind", "EQ", "preference")]))
    assert scan.cursor == "0" and scan.ids == ["a"]
    search = MemorySearch(top_k=1, filters=[Filter("@importance", "GTE", .5)], with_metadata=True, with_vector=True, with_scores=True)
    hits = [
        klyro.memory.search("hybrid", "database", search),
        klyro.memory.vector_search("hybrid", [1, 0], search),
        klyro.memory.query("hybrid", MemoryQuery(text="database", vector=[1, 0], fusion="RRF", weights=weights, top_k=1, with_scores=True)),
    ]
    for result in hits:
        assert result and result[0].id == "a" and math.isfinite(result[0].score)
        assert result[0].keyword_score is not None

    with pytest.raises(ResponseError):
        klyro.memory.add("hybrid", MemoryAdd(id="a", text="duplicate", condition="NX"))
    with pytest.raises(ResponseError):
        klyro.memory.add("hybrid", MemoryAdd(text="bad", vector=[1]))
    with pytest.raises(ResponseError):
        klyro.memory.add("hybrid", MemoryAdd(text="bad", vector=[math.nan, 0]))
    with pytest.raises(ResponseError):
        klyro.memory.info("missing")
    klyro.set("wrong", "type")
    with pytest.raises(ResponseError):
        klyro.memory.card("wrong")

    assert klyro.memory.add("hybrid", MemoryAdd(id="a", text="updated", vector=[0, 1], condition="XX")) == "a"
    assert klyro.memory.delete("hybrid", "a", "missing") == 1
    assert klyro.memory.card("hybrid") == 0
    assert klyro.memory.create("search", MemoryCreate(mode="SEARCH")) == "OK"
    generated = klyro.memory.add("search", MemoryAdd(text=""))
    assert isinstance(generated, str)
    assert klyro.memory.get("search", generated, MemoryReturn(with_vector=True)).vector is None
    with pytest.raises(ResponseError):
        klyro.memory.vector_search("search", [1, 0])
    assert klyro.memory.create("vector", MemoryCreate(mode="VECTOR", dim=2, metric="IP")) == "OK"
    raw = __import__("struct").pack("<2f", 1, .5)
    klyro.memory.add("vector", MemoryAdd(text="vector", vector=raw))
    assert len(klyro.memory.query("vector", MemoryQuery(vector=raw))) == 1


def test_option_validation(klyro: Klyro) -> None:
    with pytest.raises(ValueError):
        klyro.memory.config("x", MemoryConfig())
    with pytest.raises(ValueError):
        klyro.memory.query("x", MemoryQuery())
    with pytest.raises(ValueError):
        klyro.memory.set_metadata("x", "id", {})
