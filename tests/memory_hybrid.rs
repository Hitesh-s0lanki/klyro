//! MEM.QUERY: the fused retrieval an agent should reach for. One
//! command serves all three structures, so which indexes run is decided
//! by what the caller passes rather than by which verb they picked.

mod common;

use common::{ok, KlyroClient, KlyroServer, Value};

fn field(reply: &Value, name: &str) -> String {
    reply
        .pairs()
        .into_iter()
        .find(|(field, _)| field == name)
        .unwrap_or_else(|| panic!("no field {name} in {reply:?}"))
        .1
}

fn score(reply: &Value, name: &str) -> f64 {
    field(reply, name).parse().expect("a numeric score")
}

fn ids(reply: &Value) -> Vec<String> {
    reply.items().iter().map(|hit| field(hit, "id")).collect()
}

/// The specification's worked example. The vectors stand in for an
/// embedding model: everything about databases points one way, and the
/// developer-tools memory points elsewhere, so semantic similarity
/// means something without an encoder in the loop.
fn corpus(client: &mut KlyroClient) {
    assert_eq!(client.send("MEM.CREATE ns MODE HYBRID DIM 3"), ok());
    for (id, text, vector) in [
        (
            "m1",
            "User prefers PostgreSQL for backend projects.",
            "0.9 0.4 0.1",
        ),
        (
            "m2",
            "User is currently building a database administration tool.",
            "0.7 0.2 0.3",
        ),
        ("m3", "User likes modern developer tools.", "0.1 0.2 0.9"),
        ("m4", "User previously worked with MySQL.", "0.8 0.5 0.1"),
        (
            "m5",
            "User is building Basora, a PostgreSQL developer application.",
            "0.95 0.35 0.2",
        ),
    ] {
        let mut args = vec!["MEM.ADD", "ns", "ID", id, "TEXT", text, "FVEC", "3"];
        args.extend(vector.split(' '));
        client.call(&args);
    }
}

#[test]
fn a_text_only_query_is_a_keyword_search() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    corpus(&mut client);
    let mut found = ids(&client.call(&["MEM.QUERY", "ns", "TEXT", "PostgreSQL"]));
    found.sort();
    assert_eq!(found, vec!["m1", "m5"]);
}

#[test]
fn a_vector_only_query_is_a_semantic_search() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    corpus(&mut client);
    // Nothing here shares a word with the query, yet the database
    // memories come back and the developer-tools one does not.
    let ranked = ids(&client.send("MEM.QUERY ns FVEC 3 0.9 0.4 0.1 TOPK 3"));
    assert!(!ranked.contains(&"m3".to_string()), "got {ranked:?}");
}

#[test]
fn giving_both_fuses_the_two_rankings() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    corpus(&mut client);
    let hits = client.call(&[
        "MEM.QUERY",
        "ns",
        "TEXT",
        "What database technology is the user currently interested in?",
        "FVEC",
        "3",
        "0.9",
        "0.4",
        "0.1",
        "TOPK",
        "3",
        "WITHSCORES",
    ]);
    let ranked = ids(&hits);
    assert_eq!(ranked.len(), 3);
    // The two PostgreSQL memories are what the specification says a
    // hybrid query should surface for this question.
    assert!(ranked.contains(&"m1".to_string()), "got {ranked:?}");
    assert!(ranked.contains(&"m5".to_string()), "got {ranked:?}");
    // Every hit carries both components, whichever index found it.
    for hit in hits.items() {
        assert!(score(hit, "score") > 0.0);
        assert!(score(hit, "vector_score") > 0.0);
    }
}

#[test]
fn a_record_only_one_index_found_still_ranks() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("MEM.CREATE ns MODE HYBRID DIM 2"), ok());
    // "keyword" shares a word with the query and points away from it;
    // "semantic" shares no word and points straight at it.
    client.send("MEM.ADD ns ID keyword TEXT unmistakable FVEC 2 0 1");
    client.call(&[
        "MEM.ADD",
        "ns",
        "ID",
        "semantic",
        "TEXT",
        "nothing alike",
        "FVEC",
        "2",
        "1",
        "0",
    ]);
    let hits = client.call(&[
        "MEM.QUERY",
        "ns",
        "TEXT",
        "unmistakable",
        "FVEC",
        "2",
        "1",
        "0",
        "WITHSCORES",
    ]);
    let ranked = ids(&hits);
    assert_eq!(ranked.len(), 2, "neither candidate may be dropped");
    let keyword_hit = hits
        .items()
        .iter()
        .find(|h| field(h, "id") == "keyword")
        .unwrap();
    assert_eq!(score(keyword_hit, "vector_score"), 0.0);
}

#[test]
fn per_query_weights_override_the_index_defaults() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("MEM.CREATE ns MODE HYBRID DIM 2"), ok());
    client.call(&[
        "MEM.ADD",
        "ns",
        "ID",
        "worded",
        "TEXT",
        "distinctive phrase",
        "FVEC",
        "2",
        "0",
        "1",
    ]);
    client.call(&[
        "MEM.ADD",
        "ns",
        "ID",
        "aimed",
        "TEXT",
        "unrelated content",
        "FVEC",
        "2",
        "1",
        "0",
    ]);

    let keyword_led = client.call(&[
        "MEM.QUERY",
        "ns",
        "TEXT",
        "distinctive",
        "FVEC",
        "2",
        "1",
        "0",
        "WEIGHTS",
        "1",
        "0",
        "0",
        "0",
    ]);
    assert_eq!(ids(&keyword_led)[0], "worded");

    let vector_led = client.call(&[
        "MEM.QUERY",
        "ns",
        "TEXT",
        "distinctive",
        "FVEC",
        "2",
        "1",
        "0",
        "WEIGHTS",
        "0",
        "1",
        "0",
        "0",
    ]);
    assert_eq!(ids(&vector_led)[0], "aimed");
    // The index's own weights are unchanged by a per-query override.
    assert_eq!(field(&client.send("MEM.INFO ns"), "weights"), "(8 items)");
}

#[test]
fn index_weights_change_the_default_ranking() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("MEM.CREATE ns MODE HYBRID DIM 2"), ok());
    client.call(&[
        "MEM.ADD",
        "ns",
        "ID",
        "worded",
        "TEXT",
        "distinctive phrase",
        "FVEC",
        "2",
        "0",
        "1",
    ]);
    client.call(&[
        "MEM.ADD",
        "ns",
        "ID",
        "aimed",
        "TEXT",
        "unrelated content",
        "FVEC",
        "2",
        "1",
        "0",
    ]);

    assert_eq!(client.send("MEM.CONFIG ns WEIGHTS 1 0 0 0"), ok());
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

#[test]
fn importance_and_recency_settle_what_the_other_signals_tie() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("MEM.CREATE ns MODE SEARCH"), ok());
    client.call(&[
        "MEM.ADD",
        "ns",
        "ID",
        "dull",
        "TEXT",
        "shared wording",
        "IMPORTANCE",
        "0.1",
    ]);
    client.call(&[
        "MEM.ADD",
        "ns",
        "ID",
        "vital",
        "TEXT",
        "shared wording",
        "IMPORTANCE",
        "0.9",
    ]);
    // Identical text, so the keyword component cannot separate them.
    let hits = client.call(&[
        "MEM.QUERY",
        "ns",
        "TEXT",
        "shared wording",
        "WEIGHTS",
        "0",
        "0",
        "0",
        "1",
    ]);
    assert_eq!(ids(&hits), vec!["vital", "dull"]);
}

#[test]
fn rrf_fuses_by_rank_instead_of_by_score() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    corpus(&mut client);
    let linear = client.call(&[
        "MEM.QUERY",
        "ns",
        "TEXT",
        "PostgreSQL",
        "FVEC",
        "3",
        "0.9",
        "0.4",
        "0.1",
        "FUSION",
        "LINEAR",
        "TOPK",
        "5",
    ]);
    let rrf = client.call(&[
        "MEM.QUERY",
        "ns",
        "TEXT",
        "PostgreSQL",
        "FVEC",
        "3",
        "0.9",
        "0.4",
        "0.1",
        "FUSION",
        "RRF",
        "TOPK",
        "5",
    ]);
    // Both strategies see the same candidates; only the ordering rule
    // differs, so neither may lose one.
    let (mut a, mut b) = (ids(&linear), ids(&rrf));
    a.sort();
    b.sort();
    assert_eq!(a, b);
    assert!(client
        .send("MEM.QUERY ns TEXT x FUSION NONSENSE")
        .is_error());
}

#[test]
fn filters_narrow_a_fused_query() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    corpus(&mut client);
    client.send("MEM.SETMETA ns m1 type preference");
    client.send("MEM.SETMETA ns m5 type project");
    let hits = client.call(&[
        "MEM.QUERY",
        "ns",
        "TEXT",
        "PostgreSQL",
        "FVEC",
        "3",
        "0.9",
        "0.4",
        "0.1",
        "FILTER",
        "type",
        "EQ",
        "preference",
    ]);
    assert_eq!(ids(&hits), vec!["m1"]);
}

#[test]
fn a_query_needs_something_to_search_with() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    corpus(&mut client);
    assert_eq!(
        client.send("MEM.QUERY ns TOPK 5").error(),
        "ERR TEXT, VEC, or FVEC is required"
    );
    assert_eq!(
        client.send("MEM.QUERY ns TEXT x NONSENSE").error(),
        "ERR unexpected argument 'NONSENSE'"
    );
}

#[test]
fn a_mode_still_refuses_the_half_of_a_query_it_cannot_answer() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("MEM.CREATE kw MODE SEARCH"), ok());
    assert!(client
        .send("MEM.QUERY kw TEXT x FVEC 2 1 0")
        .error()
        .contains("stores no vectors"));
    // Text alone is fine on the same index.
    assert!(!client.send("MEM.QUERY kw TEXT x").is_error());
}
