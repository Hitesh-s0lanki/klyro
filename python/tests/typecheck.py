from typing import Any

from klyro_db import Klyro, MemoryAdd, MemoryCreate, MemoryHit, MemoryQuery

db = Klyro()
ok: str = db.memory.create("notes", MemoryCreate(dim=2))
record_id: str = db.memory.add("notes", MemoryAdd(text="hello", vector=[1, 0]))
hits: list[MemoryHit] = db.memory.query("notes", MemoryQuery(text="hello", top_k=2))
value: Any = db.get("key")
