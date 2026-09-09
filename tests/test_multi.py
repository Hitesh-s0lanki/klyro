import unittest

from klyro_helper import KlyroServer


class TestMultiValue(unittest.TestCase):
    """LPUSH/RPUSH/SADD/ZADD taking multiple values/pairs in one call."""

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

    def test_lpush_multi_pushes_each_to_head_in_turn(self):
        k = self.key("k")
        self.assertEqual(self.client.send(f"LPUSH {k} a b c"), "LEN 3\r\n")
        # Redis semantics: LPUSH k a b c -> list ends up [c, b, a]
        self.assertEqual(self.client.send(f"LRANGE {k} 0 -1"), "c\r\nb\r\na\r\nEND\r\n")

    def test_rpush_multi_pushes_in_order(self):
        k = self.key("k")
        self.assertEqual(self.client.send(f"RPUSH {k} a b c"), "LEN 3\r\n")
        self.assertEqual(self.client.send(f"LRANGE {k} 0 -1"), "a\r\nb\r\nc\r\nEND\r\n")

    def test_push_requires_at_least_one_value(self):
        k = self.key("k")
        self.assertEqual(
            self.client.send(f"LPUSH {k}"), "ERR usage: LPUSH key value [value ...]\r\n"
        )

    def test_sadd_multi_counts_only_new_members(self):
        k = self.key("k")
        self.assertEqual(self.client.send(f"SADD {k} x y z"), "ADDED 3\r\n")
        self.assertEqual(self.client.send(f"SADD {k} x y w"), "ADDED 1\r\n")
        resp = self.client.send(f"SMEMBERS {k}")
        self.assertEqual(set(resp.strip().split("\r\n")[:-1]), {"x", "y", "z", "w"})

    def test_sadd_requires_at_least_one_member(self):
        k = self.key("k")
        self.assertEqual(
            self.client.send(f"SADD {k}"), "ERR usage: SADD key member [member ...]\r\n"
        )

    def test_zadd_multi_pairs(self):
        k = self.key("k")
        self.assertEqual(self.client.send(f"ZADD {k} 100 alice 50 bob 75 carol"), "ADDED 3\r\n")
        self.assertEqual(
            self.client.send(f"ZRANGE {k} 0 -1"), "bob 50\r\ncarol 75\r\nalice 100\r\nEND\r\n"
        )

    def test_zadd_multi_pairs_counts_only_new_members(self):
        k = self.key("k")
        self.client.send(f"ZADD {k} 100 alice")
        self.assertEqual(self.client.send(f"ZADD {k} 10 alice 200 dave"), "ADDED 1\r\n")

    def test_zadd_dangling_score_is_rejected(self):
        k = self.key("k")
        self.assertEqual(
            self.client.send(f"ZADD {k} 5"),
            "ERR usage: ZADD key score member [score member ...]\r\n",
        )

    def test_zadd_non_numeric_score_is_rejected(self):
        k = self.key("k")
        self.assertEqual(
            self.client.send(f"ZADD {k} notanumber alice"),
            "ERR usage: ZADD key score member [score member ...]\r\n",
        )

    def test_zadd_too_many_pairs_is_rejected(self):
        k = self.key("k")
        pairs = " ".join(f"{i} m{i}" for i in range(130))
        self.assertEqual(
            self.client.send(f"ZADD {k} {pairs}"), "ERR too many score/member pairs\r\n"
        )

    def test_single_value_set_and_hset_still_allow_spaces(self):
        k = self.key("k")
        self.client.send(f"SET {k} hello world")
        self.assertEqual(self.client.send(f"GET {k}"), "VALUE hello world\r\n")
        self.client.send(f"HSET {k}_h name Alice Smith")
        self.assertEqual(self.client.send(f"HGET {k}_h name"), "VALUE Alice Smith\r\n")


if __name__ == "__main__":
    unittest.main()
