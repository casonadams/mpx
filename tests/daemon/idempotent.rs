use super::support::{connect, logs_until, started_with, task};

#[tokio::test]
async fn repeated_key_pastes_once() {
    let (_daemon, mut client) = started_with(task("sink", "cat", &[])).await;

    client
        .idempotency_key("paste-1")
        .paste("sink", "hello_once", false)
        .await
        .unwrap();
    client
        .idempotency_key("paste-1")
        .paste("sink", "hello_once", false)
        .await
        .unwrap();
    client.paste("sink", "barrier_a", false).await.unwrap();
    let data = logs_until(&mut client, "sink", "barrier_a").await;
    assert_eq!(data.matches("hello_once").count(), 1, "logs: {data}");
}

#[tokio::test]
async fn concurrent_repeats_of_a_key_paste_once() {
    let (daemon, mut client) = started_with(task("racer", "cat", &[])).await;

    let mut racers = Vec::new();
    for _ in 0..2 {
        let ep = daemon.endpoint.clone();
        racers.push(tokio::spawn(async move {
            let mut c = connect(&ep).await;
            c.idempotency_key("race-1")
                .paste("racer", "race_once", false)
                .await
        }));
    }
    for racer in racers {
        racer.await.unwrap().unwrap();
    }
    client.paste("racer", "barrier_b", false).await.unwrap();
    let data = logs_until(&mut client, "racer", "barrier_b").await;
    assert_eq!(data.matches("race_once").count(), 1, "logs: {data}");
}

#[tokio::test]
async fn nested_idempotent_requests_are_rejected() {
    let (_daemon, mut client) = started_with(task("nest", "cat", &[])).await;
    let err = client
        .idempotency_key("outer")
        .stream_logs(Some("nest".to_string()), None, |_| false)
        .await
        .unwrap_err();
    assert!(err.contains("Idempotent"), "error: {err}");
}
