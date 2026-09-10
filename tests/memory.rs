//! Memory index commands: creation, record CRUD, metadata, per-record
//! TTL, scanning, and how a memory key behaves under the generic
//! keyspace commands.

mod common;

use common::{int, nil, ok, KlyroServer, KlyroClient, Value, WRONGTYPE};

/// A float32 vector encoded the way a client sends one: raw
/// little-endian bytes, four per dimension.
fn vec_blob(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// `MEM.ADD ns ID id TEXT text VEC <blob>`, which needs raw bytes for
/// the vector and so can't go through the whitespace-splitting helper.
fn add(client: &mut KlyroClient, ns: &str, id: &str, text: &str, values: &[f32]) -> Value {
    client.call_bytes(&[
        b"MEM.ADD".to_vec(),
        ns.as_bytes().to_vec(),
        b"ID".to_vec(),
        id.as_bytes().to_vec(),
        b"TEXT".to_vec(),
        text.as_bytes().to_vec(),
        b"VEC".to_vec(),
        vec_blob(values),
    ])
}

/// One reply map read back as a field lookup.
fn field(reply: &Value, name: &str) -> String {
    reply
        .pairs()
        .into_iter()
        .find(|(field, _)| field == name)
        .unwrap_or_else(|| panic!("no field {name} in {reply:?}"))
        .1
}

/// The ids of a search or scan result, in rank order.
fn ids(reply: &Value) -> Vec<String> {
    reply.items().iter().map(|hit| field(hit, "id")).collect()
}

fn hybrid(client: &mut KlyroClient) {
    assert_eq!(client.send("MEM.CREATE ns MODE HYBRID DIM 3"), ok());
}

// --- creation -----------------------------------------------------

#[test]
fn create_defaults_to_hybrid_with_the_specified_weights() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("MEM.CREATE ns DIM 4"), ok());
    let info = client.send("MEM.INFO ns");
    assert_eq!(field(&info, "mode"), "HYBRID");
    assert_eq!(field(&info, "dim"), "4");
    assert_eq!(field(&info, "metric"), "COSINE");
    assert_eq!(field(&info, "halflife"), "604800");
    assert_eq!(field(&info, "records"), "0");
}

#[test]
fn create_refuses_to_replace_an_existing_index() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    hybrid(&mut client);
    add(&mut client, "ns", "a", "keep me", &[1.0, 0.0, 0.0]);
    assert_eq!(
        client.send("MEM.CREATE ns MODE HYBRID DIM 3").error(),
        "ERR memory index already exists"
    );
    // The records it already held are untouched.
    assert_eq!(client.send("MEM.CARD ns"), int(1));
}

#[test]
fn a_vector_index_needs_a_dimension() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(
        client.send("MEM.CREATE ns MODE HYBRID").error(),
        "ERR DIM is required for a VECTOR or HYBRID index"
    );
    // A keyword-only index needs none.
    assert_eq!(client.send("MEM.CREATE ns MODE SEARCH"), ok());
    assert_eq!(field(&client.send("MEM.INFO ns"), "dim"), "0");
}

#[test]
fn create_rejects_a_dimension_past_the_configured_ceiling() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert!(client
        .send("MEM.CREATE ns MODE VECTOR DIM 99999")
        .error()
        .starts_with("ERR DIM must be between 1 and mem-max-dim"));
    assert_eq!(client.send("MEM.CREATE ns MODE VECTOR DIM 0").error().is_empty(), false);
}

#[test]
fn a_dim_on_a_keyword_only_index_is_a_mistake_worth_reporting() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    // Almost always a client that meant HYBRID. Accepting it silently
    // would leave MEM.INFO reporting 0 against the number they passed.
    assert!(client
        .send("MEM.CREATE ns MODE SEARCH DIM 384")
        .error()
        .contains("did you mean MODE HYBRID?"));
    assert_eq!(client.send("MEM.CREATE ns MODE SEARCH"), ok());
    assert_eq!(field(&client.send("MEM.INFO ns"), "dim"), "0");
}

#[test]
fn topk_is_bounded_by_the_configured_ceiling() {
    let server = KlyroServer::with_config("mem-max-topk 3");
    let mut client = server.connect();
    hybrid(&mut client);
    for i in 0..5 {
        add(&mut client, "ns", &format!("r{i}"), "shared term", &[1.0, 0.0, 0.0]);
    }
    assert_eq!(client.send("MEM.SEARCH ns shared TOPK 3").items().len(), 3);
    for command in [
        "MEM.SEARCH ns shared TOPK 4",
        "MEM.VSEARCH ns FVEC 3 1 0 0 TOPK 4",
        "MEM.QUERY ns TEXT shared TOPK 4",
    ] {
        assert!(
            client.send(command).error().contains("mem-max-topk (3)"),
            "for {command}"
        );
    }
    assert!(client.send("MEM.SEARCH ns shared TOPK 0").is_error());
}

#[test]
fn record_text_is_bounded_by_the_configured_ceiling() {
    let server = KlyroServer::with_config("mem-max-text-bytes 16");
    let mut client = server.connect();
    hybrid(&mut client);
    assert!(!client
        .call(&["MEM.ADD", "ns", "ID", "a", "TEXT", "sixteen bytes ok", "FVEC", "3", "1", "0", "0"])
        .is_error());
    let refused = client.call(&[
        "MEM.ADD", "ns", "ID", "b", "TEXT", "seventeen bytes!!", "FVEC", "3", "1", "0", "0",
    ]);
    assert!(
        refused.error().contains("mem-max-text-bytes (16)"),
        "{}",
        refused.error()
    );
    assert_eq!(client.send("MEM.CARD ns"), int(1));
}

#[test]
fn a_resp3_client_gets_real_maps_back() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    hybrid(&mut client);
    add(&mut client, "ns", "a", "alpha", &[1.0, 0.0, 0.0]);
    client.send("HELLO 3");
    // Over RESP3 a record is a map type, not the flat array RESP2
    // clients unpack. Both must carry the same fields.
    let record = client.send("MEM.GET ns a WITHMETA");
    assert_eq!(field(&record, "id"), "a");
    assert_eq!(field(&record, "text"), "alpha");
    let hits = client.send("MEM.SEARCH ns alpha WITHSCORES");
    let hit = &hits.items()[0];
    assert_eq!(field(hit, "id"), "a");
    assert!(field(hit, "keyword_score").parse::<f64>().unwrap() > 0.0);
}

#[test]
fn weights_and_halflife_are_tunable_but_the_shape_is_not() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    hybrid(&mut client);
    assert_eq!(
        client.send("MEM.CONFIG ns WEIGHTS 0.2 0.6 0.1 0.1 HALFLIFE 3600"),
        ok()
    );
    let info = client.send("MEM.INFO ns");
    assert_eq!(field(&info, "halflife"), "3600");
    // Mode is not a MEM.CONFIG parameter: every posting and every
    // stored vector was built against it.
    assert!(client.send("MEM.CONFIG ns MODE SEARCH").is_error());
    assert!(client.send("MEM.CONFIG ns WEIGHTS -1 0 0 0").is_error());
}

// --- records ------------------------------------------------------

#[test]
fn add_returns_the_id_and_assigns_one_when_asked() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    hybrid(&mut client);
    assert_eq!(add(&mut client, "ns", "mem_001", "text", &[1.0, 0.0, 0.0]).text(), "mem_001");
    let assigned = client.call_bytes(&[
        b"MEM.ADD".to_vec(),
        b"ns".to_vec(),
        b"TEXT".to_vec(),
        b"no id given".to_vec(),
        b"VEC".to_vec(),
        vec_blob(&[0.0, 1.0, 0.0]),
    ]);
    assert_eq!(assigned.text(), "m1");
    assert_eq!(client.send("MEM.CARD ns"), int(2));
}

#[test]
fn get_returns_the_record_and_nil_for_a_missing_one() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    hybrid(&mut client);
    client.call(&[
        "MEM.ADD", "ns", "ID", "a", "TEXT", "User prefers PostgreSQL", "FVEC", "3", "1", "0", "0",
        "META", "type", "preference", "IMPORTANCE", "0.85",
    ]);
    let record = client.send("MEM.GET ns a WITHMETA");
    assert_eq!(field(&record, "id"), "a");
    assert_eq!(field(&record, "text"), "User prefers PostgreSQL");
    assert_eq!(field(&record, "importance"), "0.85");
    assert_eq!(field(&record, "pttl"), "-1");
    assert_eq!(client.send("MEM.GET ns missing"), nil());
}

#[test]
fn mget_answers_in_the_order_asked() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    hybrid(&mut client);
    add(&mut client, "ns", "a", "alpha", &[1.0, 0.0, 0.0]);
    add(&mut client, "ns", "b", "beta", &[0.0, 1.0, 0.0]);
    let reply = client.send("MEM.MGET ns b missing a");
    assert_eq!(reply.items().len(), 3);
    assert_eq!(field(&reply.items()[0], "id"), "b");
    assert_eq!(reply.items()[1], nil());
    assert_eq!(field(&reply.items()[2], "id"), "a");
}

#[test]
fn withvec_returns_the_stored_vector_as_float32_bytes() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("MEM.CREATE ns MODE VECTOR DIM 3 METRIC IP"), ok());
    add(&mut client, "ns", "a", "text", &[1.5, -2.5, 0.25]);
    let record = client.send("MEM.GET ns a WITHVEC");
    let vector = record
        .items()
        .chunks(2)
        .find(|pair| pair[0].text() == "vector")
        .expect("a vector field")[1]
        .bytes();
    // Inner product stores vectors unchanged, so the bytes come back
    // exactly as they were sent.
    assert_eq!(vector, vec_blob(&[1.5, -2.5, 0.25]));
}

#[test]
fn adding_the_same_id_updates_in_place() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    hybrid(&mut client);
    add(&mut client, "ns", "a", "alpha", &[1.0, 0.0, 0.0]);
    add(&mut client, "ns", "a", "beta", &[0.0, 1.0, 0.0]);
    assert_eq!(client.send("MEM.CARD ns"), int(1));
    assert_eq!(field(&client.send("MEM.GET ns a"), "text"), "beta");
    // The old text is no longer findable, and the vector slot was
    // reused rather than leaked.
    assert!(ids(&client.send("MEM.SEARCH ns alpha")).is_empty());
    assert_eq!(field(&client.send("MEM.INFO ns"), "vectors"), "1");
}

#[test]
fn nx_and_xx_gate_on_whether_the_record_exists() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    hybrid(&mut client);
    assert!(client
        .call(&["MEM.ADD", "ns", "ID", "a", "TEXT", "x", "FVEC", "3", "1", "0", "0", "XX"])
        .is_error());
    assert!(!client
        .call(&["MEM.ADD", "ns", "ID", "a", "TEXT", "x", "FVEC", "3", "1", "0", "0", "NX"])
        .is_error());
    assert!(client
        .call(&["MEM.ADD", "ns", "ID", "a", "TEXT", "y", "FVEC", "3", "1", "0", "0", "NX"])
        .is_error());
    assert_eq!(
        client
            .call(&["MEM.ADD", "ns", "ID", "a", "TEXT", "x", "FVEC", "3", "1", "0", "0", "NX", "XX"])
            .error(),
        "ERR NX and XX are mutually exclusive"
    );
}

#[test]
fn del_counts_only_the_records_that_were_there() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    hybrid(&mut client);
    add(&mut client, "ns", "a", "alpha", &[1.0, 0.0, 0.0]);
    add(&mut client, "ns", "b", "beta", &[0.0, 1.0, 0.0]);
    assert_eq!(client.send("MEM.DEL ns a missing b"), int(2));
    assert_eq!(client.send("MEM.CARD ns"), int(0));
    // The index itself survives being emptied - it still carries the
    // mode and dimension MEM.CREATE established.
    assert_eq!(client.send("TYPE ns").text(), "memory");
    assert_eq!(field(&client.send("MEM.INFO ns"), "dim"), "3");
}

#[test]
fn text_and_metadata_may_be_arbitrary_bytes() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    hybrid(&mut client);
    let text = b"line one\nline two\0with a nul\xff".to_vec();
    client.call_bytes(&[
        b"MEM.ADD".to_vec(),
        b"ns".to_vec(),
        b"ID".to_vec(),
        b"a".to_vec(),
        b"TEXT".to_vec(),
        text.clone(),
        b"VEC".to_vec(),
        vec_blob(&[1.0, 0.0, 0.0]),
        b"META".to_vec(),
        b"awkward\r\nfield".to_vec(),
        b"value\0here".to_vec(),
    ]);
    let record = client.send("MEM.GET ns a WITHMETA");
    let text_field = record
        .items()
        .chunks(2)
        .find(|pair| pair[0].text() == "text")
        .expect("a text field");
    assert_eq!(text_field[1].bytes(), text);
}

// --- metadata -----------------------------------------------------

#[test]
fn setmeta_counts_new_fields_and_delmeta_counts_removed_ones() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    hybrid(&mut client);
    add(&mut client, "ns", "a", "text", &[1.0, 0.0, 0.0]);
    assert_eq!(client.send("MEM.SETMETA ns a type preference tag db"), int(2));
    assert_eq!(client.send("MEM.SETMETA ns a type fact"), int(0));
    assert_eq!(client.send("MEM.DELMETA ns a tag absent"), int(1));
    assert_eq!(
        client.send("MEM.SETMETA ns missing type x").error(),
        "ERR no such record"
    );
    assert_eq!(
        client.send("MEM.SETMETA ns a dangling").error(),
        "ERR wrong number of arguments for 'mem.setmeta' command"
    );
}

// --- per-record TTL -----------------------------------------------

#[test]
fn a_record_expires_on_its_own_deadline() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    hybrid(&mut client);
    add(&mut client, "ns", "a", "alpha", &[1.0, 0.0, 0.0]);
    add(&mut client, "ns", "b", "beta", &[0.0, 1.0, 0.0]);
    assert_eq!(client.send("MEM.EXPIRE ns a 100"), int(1));
    assert!(field(&client.send("MEM.GET ns a"), "pttl").parse::<i64>().unwrap() > 99_000);
    // 0 clears the deadline, the way PERSIST relates to EXPIRE.
    assert_eq!(client.send("MEM.EXPIRE ns a 0"), int(1));
    assert_eq!(field(&client.send("MEM.GET ns a"), "pttl"), "-1");
    assert_eq!(client.send("MEM.EXPIRE ns missing 100"), int(0));
    // The key holding the index has its own, separate TTL.
    assert_eq!(client.send("TTL ns"), int(-1));
}

// --- scanning -----------------------------------------------------

#[test]
fn scan_pages_through_every_record_once() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    hybrid(&mut client);
    for i in 0..7 {
        add(&mut client, "ns", &format!("r{i}"), "text", &[1.0, 0.0, 0.0]);
    }
    let mut seen = Vec::new();
    let mut cursor = "0".to_string();
    loop {
        let page = client.call(&["MEM.SCAN", "ns", &cursor, "COUNT", "3"]);
        let items = page.items();
        cursor = items[0].text();
        seen.extend(items[1].list());
        if cursor == "0" {
            break;
        }
    }
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 7);
}

#[test]
fn scan_applies_metadata_filters() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    hybrid(&mut client);
    client.call(&["MEM.ADD", "ns", "ID", "a", "TEXT", "x", "FVEC", "3", "1", "0", "0", "META", "type", "preference"]);
    client.call(&["MEM.ADD", "ns", "ID", "b", "TEXT", "y", "FVEC", "3", "1", "0", "0", "META", "type", "fact"]);
    let page = client.send("MEM.SCAN ns 0 COUNT 100 FILTER type EQ preference");
    assert_eq!(page.items()[1].list(), vec!["a"]);
}

// --- the keyspace -------------------------------------------------

#[test]
fn generic_commands_treat_a_memory_index_like_any_other_key() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    hybrid(&mut client);
    add(&mut client, "ns", "a", "alpha", &[1.0, 0.0, 0.0]);

    assert_eq!(client.send("TYPE ns").text(), "memory");
    assert_eq!(client.send("EXISTS ns"), int(1));
    assert_eq!(client.send("DBSIZE"), int(1));
    assert_eq!(client.send("KEYS n*").list(), vec!["ns"]);

    // COPY deep-copies: the copy has the records and the configuration,
    // and the two then diverge.
    assert_eq!(client.send("COPY ns twin"), int(1));
    assert_eq!(client.send("MEM.CARD twin"), int(1));
    add(&mut client, "ns", "b", "beta", &[0.0, 1.0, 0.0]);
    assert_eq!(client.send("MEM.CARD twin"), int(1));
    assert_eq!(client.send("MEM.CARD ns"), int(2));

    // The copy carries the id counter too. Without it the copy would
    // start assigning "m1" again and silently overwrite the record
    // already using that id.
    assert_eq!(client.call(&["MEM.ADD", "ns", "TEXT", "assigned here"]).text(), "m1");
    assert_eq!(client.send("COPY ns second REPLACE"), int(1));
    assert_eq!(client.call(&["MEM.ADD", "second", "TEXT", "and here"]).text(), "m2");
    assert_eq!(client.send("MEM.CARD second"), int(4));
    assert_eq!(client.send("DEL second"), int(1));

    assert_eq!(client.send("RENAME twin renamed"), ok());
    assert_eq!(client.send("MEM.CARD renamed"), int(1));
    assert_eq!(client.send("EXPIRE renamed 100"), int(1));
    assert!(client.send("TTL renamed").integer() > 0);
    assert_eq!(client.send("DEL renamed"), int(1));
    assert_eq!(client.send("MEM.CARD renamed").error(), "ERR no such memory index");
}

#[test]
fn info_reports_a_memorydb_section() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    hybrid(&mut client);
    add(&mut client, "ns", "a", "alpha beta gamma", &[1.0, 0.0, 0.0]);
    let info = client.send("INFO memorydb").text();
    assert!(info.contains("memory_indexes:1"), "{info}");
    assert!(info.contains("memory_records:1"), "{info}");
    assert!(info.contains("memory_vectors:1"), "{info}");
    assert!(info.contains("memory_terms:3"), "{info}");
    // The keyspace breakdown counts it too.
    assert!(client.send("INFO keyspace").text().contains("memory:1"));
}

// --- errors -------------------------------------------------------

#[test]
fn every_command_reports_a_missing_index_the_same_way() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    for command in [
        "MEM.INFO ns",
        "MEM.CARD ns",
        "MEM.GET ns a",
        "MEM.DEL ns a",
        "MEM.SEARCH ns query",
        "MEM.SCAN ns 0",
        "MEM.EXPIRE ns a 10",
        "MEM.SETMETA ns a f v",
    ] {
        assert_eq!(
            client.send(command).error(),
            "ERR no such memory index",
            "for {command}"
        );
    }
}

#[test]
fn every_command_reports_wrongtype_against_another_type() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET plain value");
    for command in [
        "MEM.INFO plain",
        "MEM.CARD plain",
        "MEM.GET plain a",
        "MEM.DEL plain a",
        "MEM.SEARCH plain query",
        "MEM.SCAN plain 0",
        "MEM.CREATE plain MODE SEARCH",
    ] {
        assert_eq!(client.send(command).error(), WRONGTYPE, "for {command}");
    }
}

#[test]
fn a_malformed_vector_is_refused_without_storing_anything() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    hybrid(&mut client);
    assert_eq!(
        client
            .call(&["MEM.ADD", "ns", "ID", "a", "TEXT", "x", "FVEC", "2", "1", "0"])
            .error(),
        "ERR wrong vector dimension, expected 3 got 2"
    );
    assert_eq!(
        client
            .call(&["MEM.ADD", "ns", "ID", "a", "TEXT", "x", "FVEC", "3", "0", "0", "0"])
            .error(),
        "ERR a zero vector has no direction to compare under COSINE"
    );
    assert!(client
        .call_bytes(&[
            b"MEM.ADD".to_vec(),
            b"ns".to_vec(),
            b"TEXT".to_vec(),
            b"x".to_vec(),
            b"VEC".to_vec(),
            b"12345".to_vec(),
        ])
        .error()
        .contains("multiple of 4"));
    assert_eq!(client.send("MEM.CARD ns"), int(0));
    assert_eq!(field(&client.send("MEM.INFO ns"), "terms"), "0");
}

#[test]
fn a_mode_refuses_the_queries_it_cannot_answer() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("MEM.CREATE kw MODE SEARCH"), ok());
    assert_eq!(client.send("MEM.CREATE vo MODE VECTOR DIM 3"), ok());

    assert!(client
        .call(&["MEM.ADD", "kw", "TEXT", "x", "FVEC", "1", "1"])
        .error()
        .contains("stores no vectors"));
    assert!(client
        .send("MEM.SEARCH vo query")
        .error()
        .contains("does not index text"));
    assert!(client
        .send("MEM.ADD vo TEXT x")
        .error()
        .contains("needs a vector on every record"));
}

#[test]
fn missing_and_unknown_arguments_are_reported_by_name() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    hybrid(&mut client);
    assert_eq!(
        client.send("MEM.ADD ns FVEC 3 1 0 0").error(),
        "ERR TEXT is required"
    );
    assert_eq!(
        client.send("MEM.ADD ns TEXT x NONSENSE").error(),
        "ERR unexpected argument 'NONSENSE'"
    );
    // An option cut off mid-way is a syntax error, not a panic.
    assert!(client.send("MEM.ADD ns TEXT x FVEC 3 1").is_error());
    assert!(client.send("MEM.CREATE ns2 MODE").is_error());
    assert!(client.send("MEM.NONSENSE ns").is_error());
}
