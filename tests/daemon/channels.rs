use mpx::daemon::channels::Channels;
use tokio::sync::oneshot;

#[test]
fn a_signal_is_not_spent_on_waiters_that_have_gone() {
    let mut channels = Channels::default();
    let (gone, dead) = oneshot::channel();
    channels.wait("ready", gone);
    drop(dead);
    channels.signal("ready");

    let (reply, mut answer) = oneshot::channel();
    channels.wait("ready", reply);
    assert_eq!(answer.try_recv(), Ok(Ok(())), "the signal waited for it");
}

#[test]
fn a_signal_wakes_the_waiters_that_remain() {
    let mut channels = Channels::default();
    let (gone, dead) = oneshot::channel();
    channels.wait("ready", gone);
    drop(dead);
    let (reply, mut answer) = oneshot::channel();
    channels.wait("ready", reply);
    channels.signal("ready");
    assert_eq!(answer.try_recv(), Ok(Ok(())));
}
