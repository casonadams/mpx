use std::time::{Duration, Instant};

use mpx::daemon::idempotency::{Claim, IdemCache, MAX_KEYS, TTL};

fn is_run(claim: &Claim<String>) -> bool {
    matches!(claim, Claim::Run)
}

#[test]
fn first_claim_runs_and_a_repeat_waits_for_its_response() {
    let t0 = Instant::now();
    let mut cache = IdemCache::<String>::default();
    assert!(is_run(&cache.begin("k", t0)));
    let Claim::Wait(mut waiting) = cache.begin("k", t0) else {
        panic!("a repeat while running waits");
    };
    assert!(waiting.try_recv().is_err(), "no response yet");
    IdemCache::finish(&mut cache, "k", "done".to_string(), t0);
    assert_eq!(waiting.try_recv().unwrap(), Some("done".to_string()));
}

#[test]
fn finished_key_replays_its_response_within_the_ttl() {
    let t0 = Instant::now();
    let mut cache = IdemCache::<String>::default();
    cache.begin("k", t0);
    IdemCache::finish(&mut cache, "k", "done".to_string(), t0);
    let Claim::Cached(value) = cache.begin("k", t0 + TTL - Duration::from_millis(1)) else {
        panic!("repeat within the TTL is cached");
    };
    assert_eq!(value, "done");
}

#[test]
fn key_is_forgotten_after_the_ttl() {
    let t0 = Instant::now();
    let mut cache = IdemCache::<String>::default();
    cache.begin("k", t0);
    IdemCache::finish(&mut cache, "k", "done".to_string(), t0);
    assert!(is_run(&cache.begin("k", t0 + TTL)));
}

#[test]
fn abandoned_claim_releases_waiters_with_no_response() {
    let t0 = Instant::now();
    let mut cache = IdemCache::<String>::default();
    cache.begin("k", t0);
    let Claim::Wait(mut waiting) = cache.begin("k", t0) else {
        panic!("a repeat while running waits");
    };
    cache.abandon("k");
    assert_eq!(waiting.try_recv().unwrap(), None);
    assert!(is_run(&cache.begin("k", t0)), "the key can run again");
}

#[test]
fn bounded_keys_evict_the_oldest_finished_first() {
    let t0 = Instant::now();
    let mut cache = IdemCache::<String>::default();
    for i in 0..MAX_KEYS + 5 {
        let key = format!("k{i}");
        assert!(is_run(&cache.begin(&key, t0)));
        IdemCache::finish(&mut cache, &key, key.clone(), t0);
    }
    assert_eq!(cache.len(), MAX_KEYS);
    assert!(is_run(&cache.begin("k0", t0)), "oldest key was evicted");
    assert!(matches!(
        cache.begin(&format!("k{}", MAX_KEYS + 4), t0),
        Claim::Cached(_)
    ));
}

#[test]
fn running_keys_do_not_evict_a_finished_response() {
    let t0 = Instant::now();
    let mut cache = IdemCache::<String>::default();
    for i in 0..MAX_KEYS {
        assert!(is_run(&cache.begin(&format!("r{i}"), t0)));
    }
    IdemCache::finish(&mut cache, "r0", "done".to_string(), t0);
    assert!(is_run(&cache.begin("late", t0)));
    IdemCache::finish(&mut cache, "late", "late".to_string(), t0);
    assert!(
        matches!(cache.begin("r0", t0), Claim::Cached(_)),
        "a retry of a finished request is not run again"
    );
}

#[test]
fn new_keys_are_refused_while_the_bound_is_all_running() {
    let t0 = Instant::now();
    let mut cache = IdemCache::<String>::default();
    for i in 0..MAX_KEYS {
        assert!(is_run(&cache.begin(&format!("r{i}"), t0)));
    }
    assert!(matches!(cache.begin("extra", t0), Claim::Busy));
    assert!(
        matches!(cache.begin("r0", t0), Claim::Wait(_)),
        "a running key still waits"
    );
}
