# Talking to Klyro

Klyro speaks RESP, the Redis wire protocol, so **any Redis client
library works**. There is nothing Klyro-specific to install.

This document replaces the three hand-written clients that used to live
in `clients/`. They existed only because the old line protocol was
bespoke; once RESP landed they were redundant, and they were removed
rather than left in the tree speaking a protocol the server no longer
answers. They are still in git history if you want to read them.

## Verified clients

These were run against Klyro over its real socket, and all of them pass:

| Client | Language | RESP2 | RESP3 |
|---|---|---|---|
| [redis-py](https://github.com/redis/redis-py) 8.1 | Python | yes | yes |
| [go-redis](https://github.com/redis/go-redis) v9 | Go | yes | yes |
| [ioredis](https://github.com/redis/ioredis) 5 | Node.js | yes | n/a |

Other clients (Jedis, Lettuce, StackExchange.Redis, redis-rs, ...) should
work too; they are simply untested here.

## Python

```python
import redis

r = redis.Redis(host="localhost", port=7171, decode_responses=True)
r.set("greeting", "hello")
print(r.get("greeting"))

# The distributed-lock primitive
if r.set("lock:job", "token", nx=True, ex=30):
    ...
```

## Go

```go
import "github.com/redis/go-redis/v9"

r := redis.NewClient(&redis.Options{Addr: "localhost:7171"})
r.Set(ctx, "greeting", "hello", 0)
value, err := r.Get(ctx, "greeting").Result()
```

## Node.js

```js
import Redis from "ioredis";

const r = new Redis({ host: "localhost", port: 7171 });
await r.set("greeting", "hello");
console.log(await r.get("greeting"));
```

## Command line

`redis-cli` works if you have it:

```sh
redis-cli -p 7171 set greeting hello
redis-cli -p 7171 get greeting
```

Without it, `nc` still works, because Klyro accepts Redis's inline
command form. Replies come back in RESP, so they carry type markers:

```
$ nc localhost 7171
PING
+PONG
SET greeting hello
+OK
GET greeting
$5
hello
```

Quote an argument that contains spaces, as you would in a shell:

```
SET greeting "hello there"
+OK
```

## What clients cannot do yet

Every client library exposes far more of the Redis API than Klyro
implements. Calling something unimplemented returns
`ERR unknown command`, which surfaces as an exception or error in the
client. The notable absences are transactions (`MULTI`/`EXEC`),
pub/sub, scripting (`EVAL`), the blocking commands (`BLPOP`), and the
Stream, Bitmap, HyperLogLog, and Geo types. See
[redis-feature-gap.md](redis-feature-gap.md) for the full list.

Klyro also has no authentication, so leave the `password` option unset,
and do not expose the port on an untrusted network.

## Protocol details

Klyro negotiates RESP3 when a client sends `HELLO 3`, and answers RESP2
otherwise. The type-shape differences (maps for `HGETALL` and
`CONFIG GET`, sets for `SMEMBERS` and `SINTER`, doubles for `ZSCORE`,
nested pairs for `WITHSCORES`) are implemented, so a client gets the
same shapes it would from Redis. See [resp-protocol.md](resp-protocol.md).
