use super::{build_snapshot, ValidationInput, ValidationRevision, ValidationSnapshot};
use crate::RunEvent;
use std::sync::{mpsc::Sender, Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

type SnapshotBuilder =
    Arc<dyn Fn(ValidationInput) -> ValidationSnapshot + Send + Sync + 'static>;

pub(crate) struct ValidationWorker {
    mailbox: Arc<ValidationMailbox>,
    handle: Option<JoinHandle<()>>,
}

impl ValidationWorker {
    pub(crate) fn spawn(events: Sender<RunEvent>) -> Self {
        Self::spawn_with_builder(events, Arc::new(build_snapshot))
    }

    #[cfg(test)]
    pub(crate) fn spawn_test(
        events: Sender<RunEvent>,
        builder: SnapshotBuilder,
    ) -> Self {
        Self::spawn_with_builder(events, builder)
    }

    fn spawn_with_builder(events: Sender<RunEvent>, builder: SnapshotBuilder) -> Self {
        let mailbox = Arc::new(ValidationMailbox::default());
        let worker_mailbox = Arc::clone(&mailbox);
        let handle = thread::spawn(move || {
            while let Some(input) = worker_mailbox.take() {
                let snapshot = builder(input);
                if events.send(RunEvent::Validation(snapshot)).is_err() {
                    break;
                }
            }
        });
        Self {
            mailbox,
            handle: Some(handle),
        }
    }

    pub(crate) fn schedule(&self, input: ValidationInput) -> bool {
        self.mailbox.replace_with_newer(input)
    }
}

impl Drop for ValidationWorker {
    fn drop(&mut self) {
        self.mailbox.stop();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[derive(Default)]
struct ValidationMailbox {
    state: Mutex<MailboxState>,
    ready: Condvar,
}

#[derive(Default)]
struct MailboxState {
    pending: Option<ValidationInput>,
    latest_scheduled: Option<ValidationRevision>,
    stopped: bool,
}

impl ValidationMailbox {
    fn replace_with_newer(&self, input: ValidationInput) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let revision = input.revision();
        if state
            .latest_scheduled
            .is_some_and(|latest| revision <= latest)
        {
            return false;
        }
        state.latest_scheduled = Some(revision);
        state.pending = Some(input);
        self.ready.notify_one();
        true
    }

    fn take(&self) -> Option<ValidationInput> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        loop {
            if let Some(input) = state.pending.take() {
                return Some(input);
            }
            if state.stopped {
                return None;
            }
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(|error| error.into_inner());
        }
    }

    fn stop(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.stopped = true;
        state.pending = None;
        self.ready.notify_all();
    }
}
