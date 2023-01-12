use std::sync::Arc;

use crate::{
    system::{ActorSystem, Prop},
    ActorError,
};

use super::{
    handler::{ActorMailbox, MailboxReceiver},
    Actor, ActorContext, ActorPath, ActorRef, SupervisionStrategy,
};

pub(crate) struct ActorRunner<A: Actor, P: Prop<A>> {
    path: ActorPath,
    actor: P,
    receiver: MailboxReceiver<A::UserMessageType>,
}

impl<A: Actor, P: Prop<A>> ActorRunner<A, P> {
    pub fn create(path: ActorPath, actor: P) -> (Self, ActorRef<A::UserMessageType>) {
        let (sender, receiver) = ActorMailbox::<A::UserMessageType>::create();
        let actor_ref = ActorRef::new(path.clone(), sender);
        let runner = ActorRunner {
            path,
            actor,
            receiver,
        };
        (runner, actor_ref)
    }

    async fn restart(
        context: &mut ActorContext<A::UserMessageType>,
        actor: &mut A,
        error: Option<&ActorError>,
    ) -> Result<(), ActorError> {
        actor.pre_restart(context, error).await
    }

    pub async fn start(&mut self, system: ActorSystem) {
        log::debug!("Starting actor '{}'...", &self.path);

        let mut actor = self.actor.create();

        let mut ctx: ActorContext<A::UserMessageType> = ActorContext {
            path: self.path.clone(),
            system,
            receiver: Arc::from(actor.create_receive()),
        };

        let mut start_error = actor.pre_start(&mut ctx).await.err();
        if start_error.is_some() {
            let mut retries = 0;
            match A::supervision_strategy() {
                SupervisionStrategy::Stop => {
                    log::error!("Actor '{}' failed to start!", &self.path);
                }
                SupervisionStrategy::Retry(mut retry_strategy) => {
                    log::debug!(
                        "Restarting actor with retry strategy: {:?}",
                        &retry_strategy
                    );
                    while retries < retry_strategy.max_retries() && start_error.is_some() {
                        log::debug!("retries: {}", &retries);
                        if let Some(duration) = retry_strategy.next_backoff() {
                            log::debug!("Backoff for {:?}", &duration);
                            tokio::time::sleep(duration).await;
                        }
                        retries += 1;
                        start_error = Self::restart(&mut ctx, &mut actor, start_error.as_ref())
                            .await
                            .err();
                    }
                }
            }
        }

        if start_error.is_none() {
            log::debug!("Actor '{}' has started successfully.", &self.path);

            while let Some(msg) = self.receiver.recv().await {
                let r = ctx.receiver.clone();

                r.receive(&mut ctx, msg).await;
                //msg.handle(&mut actor, &mut ctx).await;
            }

            actor.post_stop(&mut ctx).await;

            log::debug!("Actor '{}' stopped.", &self.path);
        }

        self.receiver.close();
    }
}

#[cfg(test)]
mod tests {

    use crate::*;

    use super::*;

    #[derive(Clone, Debug)]
    struct TestEvent(String);

    #[derive(Clone)]
    struct NoRetryActor;

    #[async_trait]
    impl Actor for NoRetryActor {
        type UserMessageType = TestEvent;

        async fn pre_start(&mut self, ctx: &mut ActorContext<TestEvent>) -> Result<(), ActorError> {
            log::info!("Starting '{}'...", ctx.path);
            let error = std::io::Error::new(std::io::ErrorKind::Interrupted, "Some error");
            Err(ActorError::new(error))
        }
    }

    fn start_system() -> ActorSystem {
        if std::env::var("RUST_LOG").is_err() {
            std::env::set_var("RUST_LOG", "trace");
        }
        let _ = env_logger::builder().is_test(true).try_init();

        ActorSystem::new("test")
    }

    #[tokio::test]
    async fn no_retry_strategy() {
        let system = start_system();
        let path = ActorPath::from("/test/actor");
        let actor = NoRetryActor;
        let (mut runner, actor_ref) = ActorRunner::create(path, || NoRetryActor);

        runner.start(system).await;

        assert!(actor_ref.is_closed());
    }

    #[derive(Clone, Default)]
    struct RetryNoIntervalActor {
        counter: usize,
    }

    #[async_trait]
    impl Actor for RetryNoIntervalActor {
        type UserMessageType = TestEvent;

        fn supervision_strategy() -> SupervisionStrategy {
            let strategy = supervision::NoIntervalStrategy::new(5);
            SupervisionStrategy::Retry(Box::new(strategy))
        }

        async fn pre_start(&mut self, ctx: &mut ActorContext<TestEvent>) -> Result<(), ActorError> {
            log::info!("Actor '{}' started.", ctx.path);
            self.counter += 1;
            log::info!("Counter is now {}", self.counter);
            let error = std::io::Error::new(std::io::ErrorKind::Interrupted, "Some error");
            Err(ActorError::new(error))
        }

        async fn pre_restart(
            &mut self,
            ctx: &mut ActorContext<TestEvent>,
            error: Option<&ActorError>,
        ) -> Result<(), ActorError> {
            log::info!(
                "Actor '{}' is restarting due to {:#?}. Resetting counter to default",
                ctx.path,
                error
            );
            *self = Self::default();
            self.counter += 1;
            log::info!("Counter is now {}", self.counter);
            let error = std::io::Error::new(std::io::ErrorKind::Interrupted, "Restart error");
            Err(ActorError::new(error))
        }
    }

    #[tokio::test]
    async fn retry_no_interval_strategy() {
        let system = start_system();
        let path = ActorPath::from("/test/actor");
        let (mut runner, actor_ref) = ActorRunner::create(path, || RetryNoIntervalActor::default());

        runner.start(system).await;

        assert!(actor_ref.is_closed());
    }

    #[derive(Clone)]
    struct RetryExpBackoffActor {
        counter: usize,
    }

    #[async_trait]
    impl Actor for RetryExpBackoffActor {
        type UserMessageType = TestEvent;

        fn supervision_strategy() -> SupervisionStrategy {
            let strategy = supervision::ExponentialBackoffStrategy::new(5);
            SupervisionStrategy::Retry(Box::new(strategy))
        }

        async fn pre_start(&mut self, ctx: &mut ActorContext<TestEvent>) -> Result<(), ActorError> {
            log::info!("Actor '{}' started.", ctx.path);
            let error = std::io::Error::new(std::io::ErrorKind::Interrupted, "Some error");
            Err(ActorError::new(error))
        }

        async fn pre_restart(
            &mut self,
            ctx: &mut ActorContext<TestEvent>,
            error: Option<&ActorError>,
        ) -> Result<(), ActorError> {
            log::info!("Actor '{}' is restarting due to {:#?}.", ctx.path, error);
            self.counter += 1;
            log::info!("Counter is now {}", self.counter);
            let error = std::io::Error::new(std::io::ErrorKind::Interrupted, "Restart error");
            Err(ActorError::new(error))
        }
    }

    #[tokio::test]
    async fn retry_exponetial_backoff_strategy() {
        let system = start_system();
        let path = ActorPath::from("/test/actor");
        let (mut runner, actor_ref) =
            ActorRunner::create(path, || RetryExpBackoffActor { counter: 0 });

        runner.start(system).await;

        assert!(actor_ref.is_closed());
    }
}
