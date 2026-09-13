# klyro-db

Typed Python client for the Klyro in-memory database. It supports ordinary
Redis-compatible commands through redis-py and provides typed methods for all
15 current `MEM.*` commands.

```python
from klyro_db import Klyro, MemoryCreate, MemoryAdd, MemoryQuery

db = Klyro()
db.memory.create("notes", MemoryCreate(mode="HYBRID", dim=384))
record_id = db.memory.add("notes", MemoryAdd(
    text="User prefers PostgreSQL",
    vector=[0.1] * 384,
    metadata={"kind": "preference"},
))
hits = db.memory.query("notes", MemoryQuery(
    text="database preference",
    vector=[0.1] * 384,
    top_k=5,
    with_metadata=True,
    with_scores=True,
))
db.close()
```

Klyro listens on `127.0.0.1:7171` by default. Run the server separately with
`npx klyro-db`, Docker, or the native binary. The package includes a
`py.typed` marker, so type checkers use its bundled annotations.
