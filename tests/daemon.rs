mod daemon {
    #[cfg(unix)]
    mod agent_events;
    mod agents;
    mod basics;
    mod durable;
    mod env;
    mod exit_drain;
    mod idempotency;
    mod idempotency_abandon;
    mod idempotent;
    mod input;
    mod layout;
    mod lifecycle;
    mod logs;
    mod panes;
    mod pipes;
    mod pipes_core;
    mod plan;
    mod prompt;
    mod snapshot;
    mod stream;
    mod support;
    mod task_node;
    mod wait;
}
