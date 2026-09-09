import unittest

from klyro_helper import KlyroServer


class TestGenericCommands(unittest.TestCase):
    """Commands valid regardless of a key's type, using distinct keys per
    test so methods sharing one server don't interfere with each other."""

    @classmethod
    def setUpClass(cls):
        cls.server = KlyroServer()
        cls.client = cls.server.connect()

    @classmethod
    def tearDownClass(cls):
        cls.client.close()
        cls.server.kill()
        cls.server.cleanup_dump()

    def test_ping(self):
        self.assertEqual(self.client.send("PING"), "PONG\r\n")

    def test_unknown_command(self):
        self.assertEqual(self.client.send("BOGUS"), "ERR unknown command\r\n")

    def test_del(self):
        self.client.send("SET generic_del x")
        self.assertEqual(self.client.send("DEL generic_del"), "OK\r\n")
        self.assertEqual(self.client.send("DEL generic_del"), "NOT_FOUND\r\n")

    def test_del_missing_key(self):
        self.assertEqual(self.client.send("DEL generic_never_existed"), "NOT_FOUND\r\n")

    def test_expire_and_ttl(self):
        self.client.send("SET generic_ek v")
        self.assertEqual(self.client.send("EXPIRE generic_ek 100"), "OK\r\n")
        resp = self.client.send("TTL generic_ek")
        self.assertIn(resp, ("TTL 100\r\n", "TTL 99\r\n"))

    def test_expire_missing_key(self):
        self.assertEqual(self.client.send("EXPIRE generic_never_existed 5"), "NOT_FOUND\r\n")

    def test_ttl_no_expiry(self):
        self.client.send("SET generic_noexp v")
        self.assertEqual(self.client.send("TTL generic_noexp"), "TTL -1\r\n")

    def test_ttl_missing_key(self):
        self.assertEqual(self.client.send("TTL generic_never_existed"), "TTL -2\r\n")

    def test_type_string_and_missing(self):
        self.client.send("SET generic_tk v")
        self.assertEqual(self.client.send("TYPE generic_tk"), "STRING\r\n")
        self.assertEqual(self.client.send("TYPE generic_never_existed"), "NONE\r\n")

    def test_quit_closes_connection(self):
        c = self.server.connect()
        self.assertEqual(c.send("QUIT"), "BYE\r\n")
        c.close()


class TestKeyspaceWide(unittest.TestCase):
    """DBSIZE/KEYS see the whole keyspace, so this class gets its own
    dedicated, empty server rather than sharing one with other tests."""

    @classmethod
    def setUpClass(cls):
        cls.server = KlyroServer()
        cls.client = cls.server.connect()

    @classmethod
    def tearDownClass(cls):
        cls.client.close()
        cls.server.kill()
        cls.server.cleanup_dump()

    def test_dbsize_and_keys(self):
        self.assertEqual(self.client.send("DBSIZE"), "COUNT 0\r\n")
        self.client.send("SET a 1")
        self.client.send("SET b 2")
        self.assertEqual(self.client.send("DBSIZE"), "COUNT 2\r\n")
        resp = self.client.send("KEYS")
        self.assertTrue(resp.endswith("END\r\n"))
        keys = set(resp.strip().split("\r\n")[:-1])
        self.assertEqual(keys, {"a", "b"})


class TestShutdownAndSave(unittest.TestCase):
    """SHUTDOWN/SAVE stop or mutate the server, so each test gets its own
    fresh instance instead of sharing one."""

    def test_save_replies_ok(self):
        server = KlyroServer()
        client = server.connect()
        try:
            client.send("SET k v")
            self.assertEqual(client.send("SAVE"), "OK\r\n")
        finally:
            client.close()
            server.kill()
            server.cleanup_dump()

    def test_shutdown_stops_the_process(self):
        server = KlyroServer()
        client = server.connect()
        self.assertEqual(client.send("SHUTDOWN"), "SHUTTING_DOWN\r\n")
        client.close()
        server.proc.wait(timeout=3)
        self.assertEqual(server.proc.returncode, 0)
        self.assertIn("ok", server.output())
        server.cleanup_dump()


if __name__ == "__main__":
    unittest.main()
