//! Semantic retrieval: MEM.VSEARCH over the three metrics, and the
//! scan ceiling that keeps a brute-force query from stalling a
//! single-threaded server.

mod common;

use common::{ok, KlyroClient, KlyroServer, Value};

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

fn add(client: &mut KlyroClient, id: &str, text: &str, values: &[f32]) -> Value {
    client.call_bytes(&[
        b"MEM.ADD".to_vec(),
        b"ns".to_vec(),
        b"ID".to_vec(),
        id.as_bytes().to_vec(),
        b"TEXT".to_vec(),
        text.as_bytes().to_vec(),
        b"VEC".to_vec(),
        vec_blob(values),
    ])
}

fn vsearch(client: &mut KlyroClient, values: &[f32], tail: &[&str]) -> Value {
    let mut args = vec![
        b"MEM.VSEARCH".to_vec(),
        b"ns".to_vec(),
        b"VEC".to_vec(),
        vec_blob(values),
    ];
    args.extend(tail.iter().map(|a| a.as_bytes().to_vec()));
    client.call_bytes(&args)
}

#[test]
fn cosine_ranks_by_direction_not_by_magnitude() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("MEM.CREATE ns MODE VECTOR DIM 3"), ok());
    add(&mut client, "aligned", "x", &[1.0, 0.0, 0.0]);
    add(&mut client, "scaled", "y", &[50.0, 0.0, 0.0]);
    add(&mut client, "orthogonal", "z", &[0.0, 1.0, 0.0]);

    let hits = vsearch(&mut client, &[2.0, 0.0, 0.0], &["TOPK", "3", "WITHSCORES"]);
    let ranked = ids(&hits);
    // A vector fifty times longer but pointing the same way is exactly
    // as similar, so those two tie and break on id.
    assert_eq!(ranked[2], "orthogonal");
    assert_eq!(
        field(&hits.items()[0], "vector_score")
            .parse::<f64>()
            .unwrap(),
        1.0
    );
    assert!(
        field(&hits.items()[2], "vector_score")
            .parse::<f64>()
            .unwrap()
            .abs()
            < 1e-6,
        "an orthogonal vector scores zero under cosine"
    );
}

#[test]
fn l2_ranks_by_distance() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(
        client.send("MEM.CREATE ns MODE VECTOR DIM 2 METRIC L2"),
        ok()
    );
    add(&mut client, "near", "x", &[1.0, 1.0]);
    add(&mut client, "far", "y", &[9.0, 9.0]);
    assert_eq!(
        ids(&vsearch(&mut client, &[1.0, 2.0], &[])),
        vec!["near", "far"]
    );
}

#[test]
fn inner_product_rewards_magnitude() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(
        client.send("MEM.CREATE ns MODE VECTOR DIM 2 METRIC IP"),
        ok()
    );
    add(&mut client, "small", "x", &[1.0, 0.0]);
    add(&mut client, "large", "y", &[5.0, 0.0]);
    // Unlike cosine, length is part of the score here.
    assert_eq!(
        ids(&vsearch(&mut client, &[1.0, 0.0], &[])),
        vec!["large", "small"]
    );
}

#[test]
fn fvec_and_vec_describe_the_same_vector() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("MEM.CREATE ns MODE VECTOR DIM 3"), ok());
    add(&mut client, "a", "x", &[0.6, 0.8, 0.0]);
    let by_blob = vsearch(&mut client, &[0.6, 0.8, 0.0], &["WITHSCORES"]);
    let by_words = client.send("MEM.VSEARCH ns FVEC 3 0.6 0.8 0 WITHSCORES");
    assert_eq!(
        field(&by_blob.items()[0], "vector_score"),
        field(&by_words.items()[0], "vector_score")
    );
}

#[test]
fn a_query_vector_of_the_wrong_width_is_refused() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("MEM.CREATE ns MODE VECTOR DIM 3"), ok());
    add(&mut client, "a", "x", &[1.0, 0.0, 0.0]);
    assert_eq!(
        client.send("MEM.VSEARCH ns FVEC 2 1 0").error(),
        "ERR wrong vector dimension, expected 3 got 2"
    );
    assert_eq!(
        client.send("MEM.VSEARCH ns TOPK 3").error(),
        "ERR VEC or FVEC is required"
    );
}

#[test]
fn filters_apply_to_a_vector_search_too() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("MEM.CREATE ns MODE VECTOR DIM 2"), ok());
    client.send("MEM.ADD ns ID a TEXT x FVEC 2 1 0 META type preference");
    client.send("MEM.ADD ns ID b TEXT y FVEC 2 1 0 META type fact");
    assert_eq!(
        ids(&client.send("MEM.VSEARCH ns FVEC 2 1 0 FILTER type EQ fact")),
        vec!["b"]
    );
}

#[test]
fn a_keyword_only_index_refuses_a_vector_search() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("MEM.CREATE ns MODE SEARCH"), ok());
    assert!(client
        .send("MEM.VSEARCH ns FVEC 2 1 0")
        .error()
        .contains("stores no vectors"));
}

#[test]
fn a_scan_past_the_ceiling_is_refused_rather_than_truncated() {
    // Klyro is single-threaded, so an unbounded brute-force scan stalls
    // every other client. A partial answer that looked complete would
    // be worse than an error.
    let server = KlyroServer::with_config("mem-max-scan 2");
    let mut client = server.connect();
    assert_eq!(client.send("MEM.CREATE ns MODE VECTOR DIM 2"), ok());
    client.send("MEM.ADD ns ID a TEXT x FVEC 2 1 0");
    client.send("MEM.ADD ns ID b TEXT y FVEC 2 0 1");
    assert_eq!(ids(&client.send("MEM.VSEARCH ns FVEC 2 1 0")).len(), 2);

    client.send("MEM.ADD ns ID c TEXT z FVEC 2 1 1");
    let refused = client.send("MEM.VSEARCH ns FVEC 2 1 0").error();
    assert!(refused.contains("exceeds mem-max-scan (2)"), "{refused}");
    // Deleting a record brings the index back under the ceiling.
    client.send("MEM.DEL ns c");
    assert_eq!(ids(&client.send("MEM.VSEARCH ns FVEC 2 1 0")).len(), 2);
}

#[test]
fn an_expired_record_is_gone_from_vector_results() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("MEM.CREATE ns MODE VECTOR DIM 2"), ok());
    client.send("MEM.ADD ns ID a TEXT x FVEC 2 1 0 TTL 1");
    client.send("MEM.ADD ns ID b TEXT y FVEC 2 1 0");
    assert_eq!(ids(&client.send("MEM.VSEARCH ns FVEC 2 1 0")).len(), 2);
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert_eq!(ids(&client.send("MEM.VSEARCH ns FVEC 2 1 0")), vec!["b"]);
}
