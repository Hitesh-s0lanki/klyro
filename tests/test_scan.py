import unittest

from klyro_helper import KlyroServer


class TestKeysPattern(unittest.TestCase):
    """KEYS with an optional glob pattern - own dedicated server since it
    needs to see (and reason about) the whole keyspace."""

    @classmethod
    def setUpClass(cls):
        cls.server = KlyroServer()
        cls.client = cls.server.connect()
        for k in ("user:1", "user:2", "user:3", "post:1", "post:2", "session:abc"):
            cls.client.send(f"SET {k} v")

    @classmethod
    def tearDownClass(cls):
        cls.client.close()
        cls.server.kill()
        cls.server.cleanup_dump()

    def keys_matching(self, pattern):
        resp = self.client.send(f"KEYS {pattern}" if pattern else "KEYS")
        self.assertTrue(resp.endswith("END\r\n"))
        return set(resp.strip().split("\r\n")[:-1])

    def test_no_pattern_matches_everything(self):
        self.assertEqual(
            self.keys_matching(None),
            {"user:1", "user:2", "user:3", "post:1", "post:2", "session:abc"},
        )

    def test_star_prefix(self):
        self.assertEqual(self.keys_matching("user:*"), {"user:1", "user:2", "user:3"})

    def test_question_mark_matches_one_char(self):
        self.assertEqual(self.keys_matching("post:?"), {"post:1", "post:2"})

    def test_star_suffix(self):
        self.assertEqual(self.keys_matching("*:1"), {"user:1", "post:1"})

    def test_character_class(self):
        self.assertEqual(
            self.keys_matching("[us]*"), {"user:1", "user:2", "user:3", "session:abc"}
        )

    def test_negated_character_class(self):
        self.assertEqual(self.keys_matching("[^up]*"), {"session:abc"})

    def test_no_match(self):
        self.assertEqual(self.keys_matching("nomatch*"), set())

    def test_exact_match_no_wildcards(self):
        self.assertEqual(self.keys_matching("user:1"), {"user:1"})


class TestScan(unittest.TestCase):
    """SCAN's resumable cursor - own dedicated server for the same reason."""

    @classmethod
    def setUpClass(cls):
        cls.server = KlyroServer()
        cls.client = cls.server.connect()
        cls.all_keys = {"user:1", "user:2", "user:3", "post:1", "post:2", "session:abc"}
        for k in cls.all_keys:
            cls.client.send(f"SET {k} v")

    @classmethod
    def tearDownClass(cls):
        cls.client.close()
        cls.server.kill()
        cls.server.cleanup_dump()

    def scan_batch(self, cmd):
        resp = self.client.send(cmd)
        lines = resp.strip().split("\r\n")
        cursor_line = lines[-1]
        self.assertTrue(cursor_line.startswith("CURSOR "))
        next_cursor = cursor_line.split()[1]
        keys = lines[:-1]
        return keys, next_cursor

    def test_single_call_covers_everything_with_generous_count(self):
        keys, next_cursor = self.scan_batch("SCAN 0 COUNT 100")
        self.assertEqual(set(keys), self.all_keys)
        self.assertEqual(next_cursor, "0")

    def test_small_count_requires_multiple_calls_but_covers_everything(self):
        cursor = "0"
        seen = set()
        rounds = 0
        while True:
            rounds += 1
            self.assertLess(rounds, 20, "SCAN should terminate well before this many rounds")
            keys, cursor = self.scan_batch(f"SCAN {cursor} COUNT 2")
            self.assertEqual(len(set(keys) & seen), 0, "SCAN re-emitted a key mid-iteration")
            seen.update(keys)
            if cursor == "0":
                break
        self.assertEqual(seen, self.all_keys)
        self.assertGreater(rounds, 1, "COUNT 2 over 6 keys should take more than one round")

    def test_match_filters_results(self):
        keys, cursor = self.scan_batch("SCAN 0 MATCH user:* COUNT 100")
        self.assertEqual(set(keys), {"user:1", "user:2", "user:3"})
        self.assertEqual(cursor, "0")

    def test_match_and_count_together(self):
        keys, cursor = self.scan_batch("SCAN 0 COUNT 100 MATCH post:*")
        self.assertEqual(set(keys), {"post:1", "post:2"})

    def test_bad_cursor_is_rejected(self):
        resp = self.client.send("SCAN notanumber")
        self.assertEqual(resp, "ERR usage: SCAN cursor [MATCH pattern] [COUNT count]\r\n")

    def test_negative_cursor_is_rejected(self):
        resp = self.client.send("SCAN -1")
        self.assertEqual(resp, "ERR usage: SCAN cursor [MATCH pattern] [COUNT count]\r\n")

    def test_unknown_option_is_rejected(self):
        resp = self.client.send("SCAN 0 BOGUS x")
        self.assertEqual(resp, "ERR usage: SCAN cursor [MATCH pattern] [COUNT count]\r\n")

    def test_match_without_pattern_is_rejected(self):
        resp = self.client.send("SCAN 0 MATCH")
        self.assertEqual(resp, "ERR usage: SCAN cursor [MATCH pattern] [COUNT count]\r\n")

    def test_count_zero_is_rejected(self):
        resp = self.client.send("SCAN 0 COUNT 0")
        self.assertEqual(resp, "ERR usage: SCAN cursor [MATCH pattern] [COUNT count]\r\n")


if __name__ == "__main__":
    unittest.main()
