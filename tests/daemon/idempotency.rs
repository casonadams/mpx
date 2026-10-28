use mpx::daemon::idempotency::{Claim, IdemCache, MAX_KEYS, TTL_MS};

fn is_run(claim: &Claim<String>) -> bool {
    matches!(claim, Claim::Run)
}

#[test]
fn first_claim_runs_and_a_repeat_waits_for_its_response() {
    let mut cache = IdemCache::<String>::default();
    assert!(is_run(&cache.begin("k", 0)));
    let Claim::Wait(mut waiting) = cache.begin("k", 1) else {
        panic!("a repeat while running waits");
    };
    assert!(waiting.try_recv().is_err(), "no response yet");
    cache.finish("k", "done".to_string(), 2);
    assert_eq!(waiting.try_recv().unwrap(), Some("done".to_string()));
}

#[test]
fn finished_key_replays_its_response_within_the_ttl() {
    let mut cache = IdemCache::<String>::default();
    cache.begin("k", 0);
    cache.finish("k", "done".to_string(), 10);
    let Claim::Cached(value) = cache.begin("k", 10 + TTL_MS - 1) else {
        panic!("repeat within the TTL is cached");
    };
    assert_eq!(value, "done");
}

#[test]
fn key_is_forgotten_after_the_ttl() {
    let mut cache = IdemCache::<String>::default();
    cache.begin("k", 0);
    cache.finish("k", "done".to_string(), 10);
    assert!(is_run(&cache.begin("k", 10 + TTL_MS)));
}

#[test]
fn abandoned_claim_releases_waiters_with_no_response() {
    let mut cache = IdemCache::<String>::default();
    cache.begin("k", 0);
    let Claim::Wait(mut waiting) = cache.begin("k", 0) else {
        panic!("a repeat while running waits");
    };
    cache.abandon("k");
    assert_eq!(waiting.try_recv().unwrap(), None);
    assert!(is_run(&cache.begin("k", 0)), "the key can run again");
}

#[test]
fn bounded_keys_evict_the_oldest_finished_first() {
    let mut cache = IdemCache::<String>::default();
    for i in 0..MAX_KEYS + 5 {
        let key = format!("k{i}");
        assert!(is_run(&cache.begin(&key, 0)));
        cache.finish(&key, key.clone(), 0);
    }
    assert_eq!(cache.len(), MAX_KEYS);
    assert!(is_run(&cache.begin("k0", 0)), "oldest key was evicted");
    assert!(matches!(
        cache.begin(&format!("k{}", MAX_KEYS + 4), 0),
        Claim::Cached(_)
    ));
}
