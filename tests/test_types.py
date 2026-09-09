import unittest

from klyro_helper import KlyroServer


class TypeTestCase(unittest.TestCase):
    """Base class: one shared server per subclass, keys namespaced by test
    method name so methods sharing that server don't interfere."""

    @classmethod
    def setUpClass(cls):
        cls.server = KlyroServer()
        cls.client = cls.server.connect()

    @classmethod
    def tearDownClass(cls):
        cls.client.close()
        cls.server.kill()
        cls.server.cleanup_dump()

    def key(self, name):
        return f"{self._testMethodName}_{name}"


class TestString(TypeTestCase):
    def test_set_get(self):
        k = self.key("k")
        self.assertEqual(self.client.send(f"SET {k} hello world"), "OK\r\n")
        self.assertEqual(self.client.send(f"GET {k}"), "VALUE hello world\r\n")

    def test_get_missing(self):
        self.assertEqual(self.client.send(f"GET {self.key('missing')}"), "NOT_FOUND\r\n")

    def test_set_overwrites(self):
        k = self.key("k")
        self.client.send(f"SET {k} first")
        self.client.send(f"SET {k} second")
        self.assertEqual(self.client.send(f"GET {k}"), "VALUE second\r\n")

    def test_set_clears_previous_expiry(self):
        k = self.key("k")
        self.client.send(f"SET {k} v")
        self.client.send(f"EXPIRE {k} 100")
        self.client.send(f"SET {k} v2")
        self.assertEqual(self.client.send(f"TTL {k}"), "TTL -1\r\n")

    def test_incr_decr_on_missing_key_starts_at_zero(self):
        k = self.key("k")
        self.assertEqual(self.client.send(f"INCR {k}"), "VALUE 1\r\n")
        k2 = self.key("k2")
        self.assertEqual(self.client.send(f"DECR {k2}"), "VALUE -1\r\n")

    def test_incr_decr_on_existing_value(self):
        k = self.key("k")
        self.client.send(f"SET {k} 10")
        self.assertEqual(self.client.send(f"INCR {k}"), "VALUE 11\r\n")
        self.assertEqual(self.client.send(f"DECR {k}"), "VALUE 10\r\n")

    def test_incr_on_non_integer_value_is_rejected(self):
        k = self.key("k")
        self.client.send(f"SET {k} notanumber")
        self.assertEqual(self.client.send(f"INCR {k}"), "ERR value is not an integer\r\n")

    def test_incr_and_decr_preserve_ttl(self):
        k = self.key("k")
        self.client.send(f"SET {k} 5")
        self.client.send(f"EXPIRE {k} 200")
        self.client.send(f"INCR {k}")
        resp = self.client.send(f"TTL {k}")
        self.assertIn(resp, ("TTL 200\r\n", "TTL 199\r\n"))

    def test_append_to_missing_key_creates_it(self):
        k = self.key("k")
        self.assertEqual(self.client.send(f"APPEND {k} hello"), "LEN 5\r\n")
        self.assertEqual(self.client.send(f"GET {k}"), "VALUE hello\r\n")

    def test_append_extends_existing_value(self):
        k = self.key("k")
        self.client.send(f"SET {k} Hello")
        self.assertEqual(self.client.send(f"APPEND {k} ,World"), "LEN 11\r\n")
        self.assertEqual(self.client.send(f"GET {k}"), "VALUE Hello,World\r\n")

    def test_append_preserves_ttl(self):
        k = self.key("k")
        self.client.send(f"SET {k} hi")
        self.client.send(f"EXPIRE {k} 200")
        self.client.send(f"APPEND {k} there")
        resp = self.client.send(f"TTL {k}")
        self.assertIn(resp, ("TTL 200\r\n", "TTL 199\r\n"))

    def test_getrange_positive_and_negative_indices(self):
        k = self.key("k")
        self.client.send(f"SET {k} HelloWorld")
        self.assertEqual(self.client.send(f"GETRANGE {k} 0 4"), "VALUE Hello\r\n")
        self.assertEqual(self.client.send(f"GETRANGE {k} -5 -1"), "VALUE World\r\n")
        self.assertEqual(self.client.send(f"GETRANGE {k} 0 -1"), "VALUE HelloWorld\r\n")

    def test_getrange_out_of_bounds_is_empty(self):
        k = self.key("k")
        self.client.send(f"SET {k} short")
        self.assertEqual(self.client.send(f"GETRANGE {k} 100 200"), "VALUE \r\n")

    def test_getrange_on_missing_key_is_empty(self):
        self.assertEqual(self.client.send(f"GETRANGE {self.key('missing')} 0 -1"), "VALUE \r\n")

    def test_setrange_overwrites_in_place(self):
        k = self.key("k")
        self.client.send(f"SET {k} HelloWorld")
        self.assertEqual(self.client.send(f"SETRANGE {k} 5 XXXXX"), "LEN 10\r\n")
        self.assertEqual(self.client.send(f"GET {k}"), "VALUE HelloXXXXX\r\n")

    def test_setrange_extends_past_current_end(self):
        k = self.key("k")
        self.client.send(f"SET {k} Hi")
        self.assertEqual(self.client.send(f"SETRANGE {k} 5 end"), "LEN 8\r\n")
        self.assertEqual(self.client.send(f"GET {k}"), "VALUE Hi   end\r\n")

    def test_setrange_on_missing_key_pads_with_spaces(self):
        k = self.key("k")
        self.assertEqual(self.client.send(f"SETRANGE {k} 3 end"), "LEN 6\r\n")
        self.assertEqual(self.client.send(f"GET {k}"), "VALUE    end\r\n")

    def test_setrange_preserves_ttl(self):
        k = self.key("k")
        self.client.send(f"SET {k} hello")
        self.client.send(f"EXPIRE {k} 200")
        self.client.send(f"SETRANGE {k} 0 world")
        resp = self.client.send(f"TTL {k}")
        self.assertIn(resp, ("TTL 200\r\n", "TTL 199\r\n"))

    def test_wrongtype_for_new_numeric_commands(self):
        k = self.key("k")
        self.client.send(f"LPUSH {k} v")
        wrongtype = "ERR WRONGTYPE Operation against a key holding the wrong kind of value\r\n"
        self.assertEqual(self.client.send(f"INCR {k}"), wrongtype)
        self.assertEqual(self.client.send(f"APPEND {k} x"), wrongtype)
        self.assertEqual(self.client.send(f"GETRANGE {k} 0 -1"), wrongtype)
        self.assertEqual(self.client.send(f"SETRANGE {k} 0 x"), wrongtype)

    def test_large_value_round_trips_through_get(self):
        # Exercises conn_reply's heap-fallback path for replies longer
        # than its 256-byte fast-path stack buffer.
        k = self.key("k")
        big_value = "x" * 5000
        self.client.send(f"SET {k} {big_value}")
        self.assertEqual(self.client.send(f"GET {k}"), f"VALUE {big_value}\r\n")


class TestList(TypeTestCase):
    def test_push_pop_len(self):
        k = self.key("k")
        self.assertEqual(self.client.send(f"LPUSH {k} b"), "LEN 1\r\n")
        self.assertEqual(self.client.send(f"LPUSH {k} a"), "LEN 2\r\n")
        self.assertEqual(self.client.send(f"RPUSH {k} c"), "LEN 3\r\n")
        self.assertEqual(self.client.send(f"LLEN {k}"), "LEN 3\r\n")
        self.assertEqual(self.client.send(f"TYPE {k}"), "LIST\r\n")

    def test_lrange_full_and_partial(self):
        k = self.key("k")
        for v in ("a", "b", "c", "d"):
            self.client.send(f"RPUSH {k} {v}")
        self.assertEqual(self.client.send(f"LRANGE {k} 0 -1"), "a\r\nb\r\nc\r\nd\r\nEND\r\n")
        self.assertEqual(self.client.send(f"LRANGE {k} 1 2"), "b\r\nc\r\nEND\r\n")
        self.assertEqual(self.client.send(f"LRANGE {k} -2 -1"), "c\r\nd\r\nEND\r\n")

    def test_pop_missing_key(self):
        self.assertEqual(self.client.send(f"LPOP {self.key('missing')}"), "NOT_FOUND\r\n")

    def test_emptied_list_deletes_key(self):
        k = self.key("k")
        self.client.send(f"RPUSH {k} only")
        self.assertEqual(self.client.send(f"LPOP {k}"), "VALUE only\r\n")
        self.assertEqual(self.client.send(f"TYPE {k}"), "NONE\r\n")
        self.assertEqual(self.client.send(f"LPOP {k}"), "NOT_FOUND\r\n")


class TestHash(TypeTestCase):
    def test_set_get_del(self):
        k = self.key("k")
        self.assertEqual(self.client.send(f"HSET {k} name Alice"), "OK\r\n")
        self.assertEqual(self.client.send(f"HGET {k} name"), "VALUE Alice\r\n")
        self.assertEqual(self.client.send(f"HGET {k} nofield"), "NOT_FOUND\r\n")
        self.assertEqual(self.client.send(f"TYPE {k}"), "HASH\r\n")

    def test_hgetall(self):
        k = self.key("k")
        self.client.send(f"HSET {k} a 1")
        self.client.send(f"HSET {k} b 2")
        resp = self.client.send(f"HGETALL {k}")
        pairs = resp.strip().split("\r\n")[:-1]
        self.assertEqual(set(zip(pairs[0::2], pairs[1::2])), {("a", "1"), ("b", "2")})

    def test_emptied_hash_deletes_key(self):
        k = self.key("k")
        self.client.send(f"HSET {k} only field")
        self.assertEqual(self.client.send(f"HDEL {k} only"), "OK\r\n")
        self.assertEqual(self.client.send(f"TYPE {k}"), "NONE\r\n")
        self.assertEqual(self.client.send(f"HDEL {k} only"), "NOT_FOUND\r\n")


class TestSet(TypeTestCase):
    def test_add_ismember_card(self):
        k = self.key("k")
        self.assertEqual(self.client.send(f"SADD {k} x"), "ADDED 1\r\n")
        self.assertEqual(self.client.send(f"SADD {k} x"), "ADDED 0\r\n")
        self.assertEqual(self.client.send(f"SISMEMBER {k} x"), "TRUE\r\n")
        self.assertEqual(self.client.send(f"SISMEMBER {k} y"), "FALSE\r\n")
        self.assertEqual(self.client.send(f"SCARD {k}"), "LEN 1\r\n")
        self.assertEqual(self.client.send(f"TYPE {k}"), "SET\r\n")

    def test_smembers(self):
        k = self.key("k")
        self.client.send(f"SADD {k} x")
        self.client.send(f"SADD {k} y")
        resp = self.client.send(f"SMEMBERS {k}")
        self.assertEqual(set(resp.strip().split("\r\n")[:-1]), {"x", "y"})

    def test_emptied_set_deletes_key(self):
        k = self.key("k")
        self.client.send(f"SADD {k} only")
        self.assertEqual(self.client.send(f"SREM {k} only"), "OK\r\n")
        self.assertEqual(self.client.send(f"TYPE {k}"), "NONE\r\n")
        self.assertEqual(self.client.send(f"SREM {k} only"), "NOT_FOUND\r\n")


class TestZset(TypeTestCase):
    def test_add_score_card(self):
        k = self.key("k")
        self.assertEqual(self.client.send(f"ZADD {k} 100 alice"), "ADDED 1\r\n")
        self.assertEqual(self.client.send(f"ZSCORE {k} alice"), "VALUE 100\r\n")
        self.assertEqual(self.client.send(f"ZSCORE {k} nobody"), "NOT_FOUND\r\n")
        self.assertEqual(self.client.send(f"ZCARD {k}"), "LEN 1\r\n")
        self.assertEqual(self.client.send(f"TYPE {k}"), "ZSET\r\n")

    def test_zrange_ascending_by_score(self):
        k = self.key("k")
        self.client.send(f"ZADD {k} 100 alice")
        self.client.send(f"ZADD {k} 50 bob")
        self.client.send(f"ZADD {k} 75 carol")
        self.assertEqual(
            self.client.send(f"ZRANGE {k} 0 -1"), "bob 50\r\ncarol 75\r\nalice 100\r\nEND\r\n"
        )

    def test_zadd_repositions_existing_member(self):
        k = self.key("k")
        self.client.send(f"ZADD {k} 100 alice")
        self.assertEqual(self.client.send(f"ZADD {k} 5 alice"), "ADDED 0\r\n")
        self.assertEqual(self.client.send(f"ZSCORE {k} alice"), "VALUE 5\r\n")

    def test_emptied_zset_deletes_key(self):
        k = self.key("k")
        self.client.send(f"ZADD {k} 1 only")
        self.assertEqual(self.client.send(f"ZREM {k} only"), "OK\r\n")
        self.assertEqual(self.client.send(f"TYPE {k}"), "NONE\r\n")
        self.assertEqual(self.client.send(f"ZREM {k} only"), "NOT_FOUND\r\n")


class TestWrongType(TypeTestCase):
    WRONGTYPE = "ERR WRONGTYPE Operation against a key holding the wrong kind of value\r\n"

    def test_list_op_on_string_key(self):
        k = self.key("k")
        self.client.send(f"SET {k} v")
        self.assertEqual(self.client.send(f"LPUSH {k} x"), self.WRONGTYPE)

    def test_hash_op_on_string_key(self):
        k = self.key("k")
        self.client.send(f"SET {k} v")
        self.assertEqual(self.client.send(f"HSET {k} f v"), self.WRONGTYPE)

    def test_set_op_on_list_key(self):
        k = self.key("k")
        self.client.send(f"LPUSH {k} v")
        self.assertEqual(self.client.send(f"SADD {k} m"), self.WRONGTYPE)

    def test_zset_op_on_hash_key(self):
        k = self.key("k")
        self.client.send(f"HSET {k} f v")
        self.assertEqual(self.client.send(f"ZADD {k} 1 m"), self.WRONGTYPE)

    def test_set_always_overwrites_regardless_of_type(self):
        k = self.key("k")
        self.client.send(f"LPUSH {k} v")
        self.assertEqual(self.client.send(f"SET {k} now-a-string"), "OK\r\n")
        self.assertEqual(self.client.send(f"TYPE {k}"), "STRING\r\n")


if __name__ == "__main__":
    unittest.main()
