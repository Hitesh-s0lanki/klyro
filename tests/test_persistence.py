import time
import unittest

from klyro_helper import KlyroServer


class TestPersistence(unittest.TestCase):
    """Each test needs its own dump-file lifecycle, so no shared server."""

    def test_fresh_dump_path_starts_empty(self):
        server = KlyroServer()
        client = server.connect()
        try:
            self.assertEqual(client.send("DBSIZE"), "COUNT 0\r\n")
        finally:
            client.close()
            server.kill()
            server.cleanup_dump()

    def test_round_trip_across_all_types_after_sigkill(self):
        server = KlyroServer()
        client = server.connect()
        client.send("SET greeting hello persistence")
        client.send("LPUSH mylist a b c")
        client.send("HSET user name Alice")
        client.send("SADD tags fast reliable")
        client.send("ZADD board 100 alice 50 bob")
        self.assertEqual(client.send("SAVE"), "OK\r\n")
        client.close()
        server.kill()  # SIGKILL, not SHUTDOWN - proves the save already on disk survives

        reloaded = KlyroServer(port=server.port + 1, dump_path=server.dump_path)
        c2 = reloaded.connect()
        try:
            self.assertEqual(c2.send("GET greeting"), "VALUE hello persistence\r\n")
            self.assertEqual(c2.send("TYPE mylist"), "LIST\r\n")
            self.assertEqual(c2.send("LRANGE mylist 0 -1"), "c\r\nb\r\na\r\nEND\r\n")
            self.assertEqual(c2.send("TYPE user"), "HASH\r\n")
            self.assertEqual(c2.send("HGET user name"), "VALUE Alice\r\n")
            self.assertEqual(c2.send("TYPE tags"), "SET\r\n")
            resp = c2.send("SMEMBERS tags")
            self.assertEqual(set(resp.strip().split("\r\n")[:-1]), {"fast", "reliable"})
            self.assertEqual(c2.send("TYPE board"), "ZSET\r\n")
            self.assertEqual(c2.send("ZRANGE board 0 -1"), "bob 50\r\nalice 100\r\nEND\r\n")
        finally:
            c2.close()
            reloaded.kill()
            reloaded.cleanup_dump()

    def test_ttl_survives_a_restart(self):
        server = KlyroServer()
        client = server.connect()
        client.send("SET longlived v")
        client.send("EXPIRE longlived 300")
        client.send("SAVE")
        client.close()
        server.kill()

        reloaded = KlyroServer(port=server.port + 1, dump_path=server.dump_path)
        c2 = reloaded.connect()
        try:
            resp = c2.send("TTL longlived")
            ttl = int(resp.strip().split()[1])
            self.assertGreater(ttl, 290)  # 300 minus a couple of seconds' overhead
            self.assertLessEqual(ttl, 300)
        finally:
            c2.close()
            reloaded.kill()
            reloaded.cleanup_dump()

    def test_key_expired_during_downtime_is_gone_on_reload(self):
        server = KlyroServer()
        client = server.connect()
        client.send("SET shortlived x")
        client.send("EXPIRE shortlived 1")
        client.send("SAVE")
        client.close()
        time.sleep(1.2)  # let it actually expire before "restarting"
        server.kill()

        reloaded = KlyroServer(port=server.port + 1, dump_path=server.dump_path)
        c2 = reloaded.connect()
        try:
            self.assertEqual(c2.send("GET shortlived"), "NOT_FOUND\r\n")
            self.assertEqual(c2.send("TYPE shortlived"), "NONE\r\n")
        finally:
            c2.close()
            reloaded.kill()
            reloaded.cleanup_dump()

    def test_graceful_shutdown_also_saves(self):
        server = KlyroServer()
        client = server.connect()
        client.send("SET savedbyshutdown v")
        client.send("SHUTDOWN")  # no explicit SAVE - relies on shutdown's own save
        client.close()
        server.proc.wait(timeout=3)

        reloaded = KlyroServer(port=server.port + 1, dump_path=server.dump_path)
        c2 = reloaded.connect()
        try:
            self.assertEqual(c2.send("GET savedbyshutdown"), "VALUE v\r\n")
        finally:
            c2.close()
            reloaded.kill()
            reloaded.cleanup_dump()


if __name__ == "__main__":
    unittest.main()
