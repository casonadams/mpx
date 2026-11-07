//! Command sequences (`a ; b`) and the jobs that commands waiting on the daemon start.
use crate::tui::app::{App, Flow};
use crate::tui::command::{Command, ScriptCommand};
use crate::tui::frame;
use crate::tui::job::{self, Job, Work};
use std::collections::VecDeque;
use std::time::Duration;

/// What one command of a sequence does: nothing more, a branch to run next, a job to start, or
/// a quit.
enum Step {
    Next,
    Branch(Command),
    /// A job that the commands after it do not wait for.
    Background(Work),
    /// A job that the commands after it wait for.
    Wait(Work),
    Quit,
}

impl App {
    /// Commands one after the other, until one quits. A command that waits on the daemon starts a
    /// job, and the commands after it run when the job finishes; a background one lets them run at
    /// once. `if-shell -F` decides its branch here, without waiting.
    pub(super) async fn run_sequence(&mut self, commands: Vec<Command>) -> Flow {
        let mut queue = VecDeque::from(commands);
        while let Some(cmd) = queue.pop_front() {
            match self.step(cmd).await {
                Step::Next => {}
                Step::Branch(cmd) => queue.push_front(cmd),
                Step::Quit => return Flow::Quit,
                Step::Background(work) => self.spawn_job(work, Vec::new()),
                Step::Wait(work) => {
                    self.spawn_job(work, queue.drain(..).collect());
                    return Flow::Continue;
                }
            }
        }
        Flow::Continue
    }

    /// Applies a finished job: its output goes to the status row, and the commands that followed
    /// its command run.
    pub async fn finish_job(&mut self, finished: Job) -> Flow {
        match finished.outcome {
            Ok(lines) => {
                for line in lines {
                    self.say(line);
                }
            }
            Err(e) => {
                self.say(e);
            }
        }
        self.run_sequence(finished.then).await
    }

    /// Plans one command of a sequence: the job it starts, the branch it runs, or it runs now.
    async fn step(&mut self, cmd: Command) -> Step {
        match cmd {
            Command::Script(ScriptCommand::IfShell {
                format: true,
                target,
                condition,
                then,
                otherwise,
                ..
            }) => self.format_branch(target.as_deref(), &condition, *then, otherwise.map(|o| *o)),
            Command::Script(ScriptCommand::RunShell {
                background,
                target,
                delay_ms,
                command,
            }) => {
                let command = frame::expand_for(self, target.as_deref(), &command);
                let work = Work::Shell {
                    command,
                    target,
                    background,
                    delay: delay_ms.map(Duration::from_millis),
                };
                scheduled(background, work)
            }
            Command::Script(ScriptCommand::IfShell {
                background,
                target,
                condition,
                then,
                otherwise,
                ..
            }) => {
                let condition = frame::expand_for(self, target.as_deref(), &condition);
                let work = Work::If {
                    condition,
                    target,
                    then,
                    otherwise,
                };
                scheduled(background, work)
            }
            Command::Script(ScriptCommand::WaitFor { op, channel }) => {
                Step::Wait(Work::Wait { op, channel })
            }
            cmd => self.run_now(cmd).await,
        }
    }

    /// `if-shell -F`: the truth of the format picks the branch to run next.
    fn format_branch(
        &self,
        target: Option<&str>,
        condition: &str,
        then: Command,
        otherwise: Option<Command>,
    ) -> Step {
        let value = frame::expand_for(self, target, condition);
        let branch = if crate::format::truthy(&value) {
            Some(then)
        } else {
            otherwise
        };
        branch.map_or(Step::Next, Step::Branch)
    }

    /// Runs `cmd` now: a quit ends the sequence, anything else lets it go on.
    async fn run_now(&mut self, cmd: Command) -> Step {
        if Box::pin(self.run(cmd)).await == Flow::Quit {
            Step::Quit
        } else {
            Step::Next
        }
    }

    /// Starts a job for a command that waits on the daemon; `rest` are the commands that follow it.
    fn spawn_job(&self, work: Work, rest: Vec<Command>) {
        let endpoint = self.endpoint.clone();
        let jobs = self.jobs.clone();
        tokio::spawn(async move {
            let finished = job::run(&endpoint, work, rest).await;
            let _ = jobs.send(finished);
        });
    }
}

/// `work` as a background job, or one the commands after it wait for.
fn scheduled(background: bool, work: Work) -> Step {
    if background {
        Step::Background(work)
    } else {
        Step::Wait(work)
    }
}
