//! Keyword retrieval: BM25 ranking, metadata filters, and the return
//! flags. The fixture is the worked example from the product
//! specification, so the ranking these tests pin is the one the design
//! promised.

mod common;

use common::{int, ok, KlyroClient, KlyroServer, Value};

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

/// The five memories the specification's hybrid example uses.
fn corpus(client: &mut KlyroClient) {
    assert_eq!(client.send("MEM.CREATE ns MODE SEARCH"), ok());
    for (id, text, kind) in [
        ("m1", "User prefers PostgreSQL for backend projects.", "preference"),
        ("m2", "User is currently building a database administration tool.", "project"),
        ("m3", "User likes modern developer tools.", "preference"),
        ("m4", "User previously worked with MySQL.", "fact"),
        ("m5", "User is building Basora, a PostgreSQL developer application.", "project"),
    ] {
        client.call(&[
            "MEM.ADD", "ns", "ID", id, "TEXT", text, "META", "type", kind,
        ]);
    }
}

#[test]
fn search_finds_only_records_holding_the_term() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    corpus(&mut client);
    let mut found = ids(&client.send("MEM.SEARCH ns PostgreSQL"));
    found.sort();
    assert_eq!(found, vec!["m1", "m5"]);
}

#[test]
fn search_is_case_insensitive_and_ignores_punctuation() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    corpus(&mut client);
    assert_eq!(
        ids(&client.send("MEM.SEARCH ns postgresql")).len(),
        2,
        "a lowercased query must match a capitalised term"
    );
    assert_eq!(ids(&client.send("MEM.SEARCH ns MySQL.")), vec!["m4"]);
}

#[test]
fn a_multi_word_query_unions_its_terms_and_ranks_by_overlap() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    corpus(&mut client);
    // "building" and "developer" both appear in m5, one each in m2/m3.
    // The whole query is one argument: RESP length-prefixes it, so a
    // multi-word query never has to be quoted or escaped.
    let ranked = ids(&client.call(&["MEM.SEARCH", "ns", "building developer"]));
    assert_eq!(ranked[0], "m5", "the record matching both terms ranks first");
    assert!(ranked.len() > 1);
}

#[test]
fn a_term_in_every_record_carries_almost_no_signal() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    corpus(&mut client);
    let hits = client.send("MEM.SEARCH ns user");
    assert_eq!(hits.items().len(), 5);
    for hit in hits.items() {
        let score: f64 = field(hit, "score").parse().unwrap();
        assert!(score < 0.2, "a term in all five records scored {score}");
    }
}

#[test]
fn stopwords_and_single_characters_match_nothing() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    corpus(&mut client);
    assert!(ids(&client.send("MEM.SEARCH ns the")).is_empty());
    assert!(ids(&client.send("MEM.SEARCH ns a")).is_empty());
}

#[test]
fn identifiers_survive_tokenization_whole() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("MEM.CREATE ns MODE SEARCH"), ok());
    client.call(&["MEM.ADD", "ns", "ID", "a", "TEXT", "failed with error MAX_RETRIES on mem_001"]);
    client.call(&["MEM.ADD", "ns", "ID", "b", "TEXT", "retries are configurable"]);
    assert_eq!(ids(&client.send("MEM.SEARCH ns mem_001")), vec!["a"]);
    assert_eq!(ids(&client.send("MEM.SEARCH ns MAX_RETRIES")), vec!["a"]);
}

#[test]
fn topk_caps_the_answer_without_changing_the_ranking() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    corpus(&mut client);
    let all = ids(&client.send("MEM.SEARCH ns user"));
    let two = ids(&client.send("MEM.SEARCH ns user TOPK 2"));
    assert_eq!(two, all[..2].to_vec());
}

#[test]
fn filters_narrow_a_result_set_by_metadata() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    corpus(&mut client);
    assert_eq!(
        ids(&client.send("MEM.SEARCH ns PostgreSQL FILTER type EQ preference")),
        vec!["m1"]
    );
    assert_eq!(
        ids(&client.send("MEM.SEARCH ns PostgreSQL FILTER type EQ project")),
        vec!["m5"]
    );
    // Clauses are ANDed, so an impossible pair returns nothing.
    assert!(ids(&client.send(
        "MEM.SEARCH ns PostgreSQL FILTER type EQ preference FILTER type EQ project"
    ))
    .is_empty());
}

#[test]
fn filters_reach_the_record_itself_through_an_at_prefix() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("MEM.CREATE ns MODE SEARCH"), ok());
    client.call(&["MEM.ADD", "ns", "ID", "high", "TEXT", "database tuning", "IMPORTANCE", "0.9"]);
    client.call(&["MEM.ADD", "ns", "ID", "low", "TEXT", "database trivia", "IMPORTANCE", "0.1"]);
    assert_eq!(
        ids(&client.send("MEM.SEARCH ns database FILTER @importance GTE 0.5")),
        vec!["high"]
    );
    assert_eq!(
        ids(&client.send("MEM.SEARCH ns database FILTER @text CONTAINS trivia")),
        vec!["low"]
    );
    assert_eq!(
        ids(&client.send("MEM.SEARCH ns database FILTER @id IN high,missing")),
        vec!["high"]
    );
}

#[test]
fn numeric_metadata_compares_as_numbers_not_as_text() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("MEM.CREATE ns MODE SEARCH"), ok());
    client.call(&["MEM.ADD", "ns", "ID", "a", "TEXT", "shared term", "META", "rank", "42"]);
    client.call(&["MEM.ADD", "ns", "ID", "b", "TEXT", "shared term", "META", "rank", "100"]);
    // As text "42" sorts after "100"; as numbers it does not.
    assert_eq!(ids(&client.send("MEM.SEARCH ns shared FILTER rank GT 50")), vec!["b"]);
    assert_eq!(ids(&client.send("MEM.SEARCH ns shared FILTER rank LT 50")), vec!["a"]);
}

#[test]
fn an_expired_record_leaves_the_results_immediately() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("MEM.CREATE ns MODE SEARCH"), ok());
    client.call(&["MEM.ADD", "ns", "ID", "a", "TEXT", "ephemeral note", "TTL", "1"]);
    client.call(&["MEM.ADD", "ns", "ID", "b", "TEXT", "ephemeral fact"]);
    assert_eq!(ids(&client.send("MEM.SEARCH ns ephemeral")).len(), 2);
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert_eq!(ids(&client.send("MEM.SEARCH ns ephemeral")), vec!["b"]);
}

#[test]
fn return_flags_choose_what_a_hit_carries() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    corpus(&mut client);
    let plain = &client.send("MEM.SEARCH ns MySQL").items()[0].pairs();
    let names: Vec<&str> = plain.iter().map(|(f, _)| f.as_str()).collect();
    assert!(names.contains(&"text"), "text is returned by default");
    assert!(!names.contains(&"meta"));

    let flagged = client.send("MEM.SEARCH ns MySQL NOTEXT WITHMETA WITHSCORES");
    let hit = &flagged.items()[0];
    let names: Vec<(String, String)> = hit.pairs();
    let names: Vec<&str> = names.iter().map(|(f, _)| f.as_str()).collect();
    assert!(!names.contains(&"text"), "NOTEXT drops it");
    assert!(names.contains(&"meta"));
    assert_eq!(field(hit, "keyword_score"), field(hit, "score"));
}

#[test]
fn searching_an_empty_index_returns_an_empty_array() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("MEM.CREATE ns MODE SEARCH"), ok());
    assert_eq!(client.send("MEM.SEARCH ns anything").items().len(), 0);
    client.call(&["MEM.ADD", "ns", "ID", "a", "TEXT", "something"]);
    assert_eq!(client.send("MEM.SEARCH ns nothingatall").items().len(), 0);
}

#[test]
fn deleting_a_record_removes_it_from_the_keyword_index() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    corpus(&mut client);
    assert_eq!(client.send("MEM.DEL ns m1"), int(1));
    assert_eq!(ids(&client.send("MEM.SEARCH ns PostgreSQL")), vec!["m5"]);
    assert!(ids(&client.send("MEM.SEARCH ns prefers")).is_empty());
}
