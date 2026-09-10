//! A memory index across a restart. What a dump carries is the
//! configuration and the records; the keyword and vector indexes are
//! rebuilt from those on load, so these tests check the rebuild as much
//! as the round trip.

mod common;

use common::{int, ok, KlyroServer, Value};

fn vec_blob(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn field(reply: &Value, name: &str) -> String {
    reply
        .pairs()
        .into_iter()
        .find(|(field, _)| field == name)
        .unwrap_or_else(|| panic!("no field {name} in {reply:?}"))
        .1
}

fn ids(reply: &Value) -> Vec<String> {
    reply.items().iter().map(|hit| field(hit, "id")).collect()
}

#[test]
fn configuration_and_records_survive_a_save_and_reload() {
    let mut server = KlyroServer::new();
    {
        let mut client = server.connect();
        client.send(
            "MEM.CREATE ns MODE HYBRID DIM 3 METRIC L2 WEIGHTS 0.4 0.4 0.1 0.1 HALFLIFE 3600",
        );
        client.call_bytes(&[
            b"MEM.ADD".to_vec(),
            b"ns".to_vec(),
            b"ID".to_vec(),
            b"a".to_vec(),
            b"TEXT".to_vec(),
            b"User prefers PostgreSQL for backend projects.".to_vec(),
            b"VEC".to_vec(),
            vec_blob(&[1.5, -2.5, 0.25]),
            b"META".to_vec(),
            b"type".to_vec(),
            b"preference".to_vec(),
            b"IMPORTANCE".to_vec(),
            b"0.85".to_vec(),
        ]);
        client.call(&[
            "MEM.ADD",
            "ns",
            "ID",
            "b",
            "TEXT",
            "User worked with MySQL",
            "FVEC",
            "3",
            "0",
            "1",
            "0",
        ]);
    }
    server.shutdown();

    let mut reloaded = KlyroServer::reload(server.dump_path.clone());
    {
        let mut client = reloaded.connect();
        let info = client.send("MEM.INFO ns");
        assert_eq!(field(&info, "mode"), "HYBRID");
        assert_eq!(field(&info, "dim"), "3");
        assert_eq!(field(&info, "metric"), "L2");
        assert_eq!(field(&info, "halflife"), "3600");
        // Weights are what a fused query ranks by, so a restart that
        // quietly reset them to the defaults would change results
        // without changing anything a client can see.
        assert_eq!(
            info.pairs()
                .into_iter()
                .filter(|(f, _)| f == "weights")
                .count(),
            1
        );
        let weights = client.send("MEM.INFO ns");
        let weights = weights
            .items()
            .chunks(2)
            .find(|pair| pair[0].text() == "weights")
            .expect("a weights field")[1]
            .pairs();
        assert_eq!(
            weights,
            vec![
                ("importance".to_string(), "0.1".to_string()),
                ("keyword".to_string(), "0.4".to_string()),
                ("recency".to_string(), "0.1".to_string()),
                ("vector".to_string(), "0.4".to_string()),
            ]
        );
        assert_eq!(field(&info, "records"), "2");
        assert_eq!(field(&info, "vectors"), "2");

        let record = client.send("MEM.GET ns a WITHMETA WITHVEC");
        assert_eq!(
            field(&record, "text"),
            "User prefers PostgreSQL for backend projects."
        );
        assert_eq!(field(&record, "importance"), "0.85");
        // L2 stores vectors unchanged, so these are the bytes sent.
        let vector = record
            .items()
            .chunks(2)
            .find(|pair| pair[0].text() == "vector")
            .expect("a vector field")[1]
            .bytes();
        assert_eq!(vector, vec_blob(&[1.5, -2.5, 0.25]));

        // The keyword index was rebuilt, not stored.
        assert_eq!(ids(&client.send("MEM.SEARCH ns PostgreSQL")), vec!["a"]);
        assert_eq!(ids(&client.send("MEM.SEARCH ns MySQL")), vec!["b"]);
    }
    reloaded.kill();
    reloaded.cleanup_dump();
}

#[test]
fn a_keyword_only_index_reloads_without_vectors() {
    let mut server = KlyroServer::new();
    {
        let mut client = server.connect();
        client.send("MEM.CREATE ns MODE SEARCH");
        client.call(&["MEM.ADD", "ns", "ID", "a", "TEXT", "no vectors here at all"]);
    }
    server.shutdown();

    let mut reloaded = KlyroServer::reload(server.dump_path.clone());
    {
        let mut client = reloaded.connect();
        let info = client.send("MEM.INFO ns");
        assert_eq!(field(&info, "mode"), "SEARCH");
        assert_eq!(field(&info, "dim"), "0");
        assert_eq!(field(&info, "vectors"), "0");
        assert_eq!(ids(&client.send("MEM.SEARCH ns vectors")), vec!["a"]);
    }
    reloaded.kill();
    reloaded.cleanup_dump();
}

#[test]
fn binary_text_and_metadata_survive_the_round_trip() {
    let mut server = KlyroServer::new();
    let text = b"line one\nline two\0with a nul\xff".to_vec();
    {
        let mut client = server.connect();
        client.send("MEM.CREATE ns MODE SEARCH");
        client.call_bytes(&[
            b"MEM.ADD".to_vec(),
            b"ns".to_vec(),
            b"ID".to_vec(),
            b"a".to_vec(),
            b"TEXT".to_vec(),
            text.clone(),
            b"META".to_vec(),
            b"awkward\r\nfield".to_vec(),
            b"value\0here".to_vec(),
        ]);
    }
    server.shutdown();

    let mut reloaded = KlyroServer::reload(server.dump_path.clone());
    {
        let mut client = reloaded.connect();
        let record = client.send("MEM.GET ns a WITHMETA");
        let stored = record
            .items()
            .chunks(2)
            .find(|pair| pair[0].text() == "text")
            .expect("a text field")[1]
            .bytes();
        assert_eq!(stored, text);
    }
    reloaded.kill();
    reloaded.cleanup_dump();
}

#[test]
fn a_record_ttl_is_an_absolute_deadline_across_a_restart() {
    let mut server = KlyroServer::new();
    {
        let mut client = server.connect();
        client.send("MEM.CREATE ns MODE SEARCH");
        client.call(&[
            "MEM.ADD",
            "ns",
            "ID",
            "long",
            "TEXT",
            "still here",
            "TTL",
            "3600",
        ]);
        client.call(&[
            "MEM.ADD",
            "ns",
            "ID",
            "short",
            "TEXT",
            "gone soon",
            "TTL",
            "1",
        ]);
    }
    server.shutdown();
    std::thread::sleep(std::time::Duration::from_millis(1100));

    let mut reloaded = KlyroServer::reload(server.dump_path.clone());
    {
        let mut client = reloaded.connect();
        // Downtime counts against the deadline, so the short one is
        // already gone while the long one keeps most of its TTL.
        assert_eq!(client.send("MEM.GET ns short"), common::nil());
        assert!(
            field(&client.send("MEM.GET ns long"), "pttl")
                .parse::<i64>()
                .unwrap()
                > 3_500_000
        );
    }
    reloaded.kill();
    reloaded.cleanup_dump();
}

#[test]
fn an_assigned_id_is_never_reissued_after_a_reload() {
    let mut server = KlyroServer::new();
    {
        let mut client = server.connect();
        client.send("MEM.CREATE ns MODE SEARCH");
        assert_eq!(
            client.call(&["MEM.ADD", "ns", "TEXT", "first"]).text(),
            "m1"
        );
        assert_eq!(
            client.call(&["MEM.ADD", "ns", "TEXT", "second"]).text(),
            "m2"
        );
        assert_eq!(client.send("MEM.DEL ns m2"), int(1));
    }
    server.shutdown();

    let mut reloaded = KlyroServer::reload(server.dump_path.clone());
    {
        let mut client = reloaded.connect();
        // m2 was deleted, but its id must not come back around: an
        // agent may still hold a reference to it.
        assert_eq!(
            client.call(&["MEM.ADD", "ns", "TEXT", "third"]).text(),
            "m3"
        );
    }
    reloaded.kill();
    reloaded.cleanup_dump();
}

#[test]
fn an_empty_index_survives_with_its_configuration() {
    let mut server = KlyroServer::new();
    {
        let mut client = server.connect();
        assert_eq!(
            client.send("MEM.CREATE ns MODE VECTOR DIM 8 METRIC IP"),
            ok()
        );
    }
    server.shutdown();

    let mut reloaded = KlyroServer::reload(server.dump_path.clone());
    {
        let mut client = reloaded.connect();
        let info = client.send("MEM.INFO ns");
        assert_eq!(field(&info, "mode"), "VECTOR");
        assert_eq!(field(&info, "dim"), "8");
        assert_eq!(field(&info, "metric"), "IP");
        assert_eq!(field(&info, "records"), "0");
    }
    reloaded.kill();
    reloaded.cleanup_dump();
}

#[test]
fn weights_set_at_runtime_survive_a_restart() {
    let mut server = KlyroServer::new();
    {
        let mut client = server.connect();
        client.send("MEM.CREATE ns MODE HYBRID DIM 2");
        client.send("MEM.CONFIG ns WEIGHTS 0.9 0.05 0.03 0.02 HALFLIFE 60");
        client.send("MEM.ADD ns ID worded TEXT distinctive FVEC 2 0 1");
        client.send("MEM.ADD ns ID aimed TEXT unrelated FVEC 2 1 0");
    }
    server.shutdown();

    let mut reloaded = KlyroServer::reload(server.dump_path.clone());
    {
        let mut client = reloaded.connect();
        assert_eq!(field(&client.send("MEM.INFO ns"), "halflife"), "60");
        // The proof that matters is the ranking, not the reported
        // number: a keyword-dominant index must still rank that way.
        let hits = client.call(&[
            "MEM.QUERY",
            "ns",
            "TEXT",
            "distinctive",
            "FVEC",
            "2",
            "1",
            "0",
        ]);
        assert_eq!(ids(&hits)[0], "worded");
    }
    reloaded.kill();
    reloaded.cleanup_dump();
}

/// Writes a dump by hand and starts a server on it. Lets a test pin
/// what happens to input no Klyro ever wrote - an older version, or a
/// file someone edited.
///
/// The counter, rather than anything derived from `contents`: tests run
/// in parallel, and two dumps that happen to be the same length would
/// otherwise share a file and race.
fn from_dump(contents: &str) -> KlyroServer {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "klyro_handwritten_{}_{}.dump",
        std::process::id(),
        serial
    ));
    std::fs::write(&path, contents).expect("write dump");
    KlyroServer::reload(path)
}

#[test]
fn a_version_2_dump_still_loads() {
    // Version 3 only adds the MEMORY record, so a dump written before
    // memory indexes existed has to keep loading unchanged.
    let mut server = from_dump("KLYRO-DUMP 2\nSTRING -1\n8\ngreeting\n5\nhello\n");
    {
        let mut client = server.connect();
        assert_eq!(client.send("GET greeting"), common::bulk("hello"));
        assert_eq!(client.send("DBSIZE"), int(1));
        // And a memory index can be created alongside it.
        assert_eq!(client.send("MEM.CREATE ns MODE SEARCH"), ok());
    }
    server.kill();
    server.cleanup_dump();
}

#[test]
fn a_hand_edited_dump_cannot_smuggle_in_an_out_of_range_importance() {
    // The command layer bounds importance, but a dump is a file on
    // disk. Fusion multiplies by this, so an unclamped 9.0 would let
    // one record outrank everything for good.
    let mut server = from_dump(concat!(
        "KLYRO-DUMP 3\n",
        "MEMORY -1 1\n",
        "2\nns\n",
        "SEARCH 0 COSINE 0.35 0.5 0.1 0.05 604800 2\n",
        "1\na\n",
        "7\nsmuggle\n",
        "1000 1000 9.0 -1 0 0\n",
    ));
    {
        let mut client = server.connect();
        assert_eq!(field(&client.send("MEM.GET ns a"), "importance"), "1");
        assert_eq!(ids(&client.send("MEM.SEARCH ns smuggle")), vec!["a"]);
    }
    server.kill();
    server.cleanup_dump();
}

#[test]
fn a_truncated_dump_loses_only_what_follows_it() {
    // A record cut off mid-stream leaves the reader out of step, so
    // everything after it is untrustworthy - but what came before is
    // still good, and the server must start rather than refuse to.
    let mut server = from_dump(concat!(
        "KLYRO-DUMP 3\n",
        "STRING -1\n5\nfirst\n2\nok\n",
        "MEMORY -1 4\n",
        "2\nns\n",
        "SEARCH 0 COSINE 0.35 0.5 0.1 0.05 604800 2\n",
        "1\na\n",
        "5\nalpha\n",
    ));
    {
        let mut client = server.connect();
        assert_eq!(client.send("GET first"), common::bulk("ok"));
        assert!(!client.send("PING").is_error());
    }
    server.kill();
    server.cleanup_dump();
}
