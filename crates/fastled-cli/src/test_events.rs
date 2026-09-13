//! FastLED production-test event priority and enabled-source policy.

use kernal_api::async_engine::{self, Deadline, PeriodicTimer, Receiver, UnboundedReceiver};
use std::future::{poll_fn, Future};
use std::pin::Pin;
use std::task::Poll;

use crate::server::TestEvent;
use crate::test_mode::TestCommandEvent;

#[derive(Debug)]
pub(crate) enum TestWake {
    TotalTimeout,
    ReadyTimeout,
    Interrupted,
    Liveness,
    Viewer(Option<TestEvent>),
    Command(Option<TestCommandEvent>),
}

pub(crate) struct TestEventSources<'a, F> {
    pub interrupt: Pin<&'a mut F>,
    pub liveness: &'a mut PeriodicTimer,
    pub viewer: &'a mut UnboundedReceiver<TestEvent>,
    pub commands: &'a mut Receiver<TestCommandEvent>,
}

impl<F: Future<Output = std::io::Result<()>>> TestEventSources<'_, F> {
    /// Preserve the production test's priority: total deadline, readiness
    /// deadline while unready, interrupt, liveness, viewer, then command events.
    /// Disabled sources are not polled and cannot consume a queued event.
    pub async fn next(
        &mut self,
        total_deadline: Deadline,
        readiness_deadline: Option<Deadline>,
        commands_open: bool,
    ) -> TestWake {
        let mut total = std::pin::pin!(async_engine::sleep_until(total_deadline));
        let mut readiness = std::pin::pin!(async_engine::sleep_until(
            readiness_deadline.unwrap_or(total_deadline)
        ));
        let mut tick = std::pin::pin!(self.liveness.tick());
        let mut viewer = std::pin::pin!(self.viewer.recv());
        let mut command = std::pin::pin!(self.commands.recv());
        poll_fn(|context| {
            if total.as_mut().poll(context).is_ready() {
                return Poll::Ready(TestWake::TotalTimeout);
            }
            if readiness_deadline.is_some() && readiness.as_mut().poll(context).is_ready() {
                return Poll::Ready(TestWake::ReadyTimeout);
            }
            if self.interrupt.as_mut().poll(context).is_ready() {
                return Poll::Ready(TestWake::Interrupted);
            }
            if tick.as_mut().poll(context).is_ready() {
                return Poll::Ready(TestWake::Liveness);
            }
            if let Poll::Ready(event) = viewer.as_mut().poll(context) {
                return Poll::Ready(TestWake::Viewer(event));
            }
            if commands_open {
                if let Poll::Ready(event) = command.as_mut().poll(context) {
                    return Poll::Ready(TestWake::Command(event));
                }
            }
            Poll::Pending
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn terminal_conditions_precede_other_ready_events() {
        async_engine::RuntimeBuilder::current_thread()
            .enable_all()
            .build()
            .unwrap()
            .run(async {
                for expected in ["total", "ready", "interrupt"] {
                    let (viewer_tx, mut viewer) = async_engine::unbounded_channel();
                    viewer_tx.send(TestEvent::Ready).unwrap();
                    let (command_tx, mut commands) = async_engine::channel(1);
                    command_tx
                        .send(TestCommandEvent::Start { index: 0 })
                        .await
                        .unwrap();
                    let mut liveness = PeriodicTimer::new(Duration::from_secs(1)).unwrap();
                    let mut interrupt = std::pin::pin!(std::future::ready(Ok(())));
                    let total = Deadline::after(if expected == "total" {
                        Duration::ZERO
                    } else {
                        Duration::from_secs(1)
                    });
                    let readiness =
                        (expected != "interrupt").then(|| Deadline::after(Duration::ZERO));
                    // Let the timer driver advance past its scheduling
                    // granularity so these timers really are ready on poll.
                    async_engine::sleep(Duration::from_millis(5)).await;
                    let event = TestEventSources {
                        interrupt: interrupt.as_mut(),
                        liveness: &mut liveness,
                        viewer: &mut viewer,
                        commands: &mut commands,
                    }
                    .next(total, readiness, true)
                    .await;
                    assert!(matches!(
                        (expected, event),
                        ("total", TestWake::TotalTimeout)
                            | ("ready", TestWake::ReadyTimeout)
                            | ("interrupt", TestWake::Interrupted)
                    ));
                }
            });
    }

    #[test]
    fn liveness_then_viewer_then_command_preserves_priority() {
        async_engine::RuntimeBuilder::current_thread()
            .enable_all()
            .build()
            .unwrap()
            .run(async {
                let (viewer_tx, mut viewer) = async_engine::unbounded_channel();
                viewer_tx.send(TestEvent::Ready).unwrap();
                let (command_tx, mut commands) = async_engine::channel(1);
                command_tx
                    .send(TestCommandEvent::Start { index: 7 })
                    .await
                    .unwrap();
                let mut liveness = PeriodicTimer::new(Duration::from_secs(1)).unwrap();
                async_engine::sleep(Duration::from_millis(5)).await;
                let mut interrupt = std::pin::pin!(std::future::pending());
                let mut sources = TestEventSources {
                    interrupt: interrupt.as_mut(),
                    liveness: &mut liveness,
                    viewer: &mut viewer,
                    commands: &mut commands,
                };
                let total = Deadline::after(Duration::from_secs(1));
                assert!(matches!(
                    sources.next(total, None, true).await,
                    TestWake::Liveness
                ));
                assert!(matches!(
                    sources.next(total, None, true).await,
                    TestWake::Viewer(Some(TestEvent::Ready))
                ));
                assert!(matches!(
                    sources.next(total, None, true).await,
                    TestWake::Command(Some(TestCommandEvent::Start { index: 7 }))
                ));
            });
    }

    #[test]
    fn disabled_command_source_does_not_consume_queued_events() {
        async_engine::RuntimeBuilder::current_thread()
            .enable_all()
            .build()
            .unwrap()
            .run(async {
                let (_viewer_tx, mut viewer) = async_engine::unbounded_channel();
                let (command_tx, mut commands) = async_engine::channel(1);
                command_tx
                    .send(TestCommandEvent::Start { index: 3 })
                    .await
                    .unwrap();
                let mut liveness = PeriodicTimer::new(Duration::from_secs(1)).unwrap();
                liveness.tick().await;
                let mut interrupt = std::pin::pin!(std::future::pending());
                let mut sources = TestEventSources {
                    interrupt: interrupt.as_mut(),
                    liveness: &mut liveness,
                    viewer: &mut viewer,
                    commands: &mut commands,
                };
                let total = Deadline::after(Duration::from_secs(1));
                assert!(async_engine::timeout(
                    Duration::from_millis(5),
                    sources.next(total, None, false)
                )
                .await
                .is_err());
                assert!(matches!(
                    sources.next(total, None, true).await,
                    TestWake::Command(Some(TestCommandEvent::Start { index: 3 }))
                ));
            });
    }
}
