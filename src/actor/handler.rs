use std::marker::PhantomData;

use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};

use crate::actor::{ActorContext, Handler, Message};

use super::{Actor, ActorError};

#[async_trait]
pub trait MessageHandler<A: Actor>: Send + Sync {
    async fn handle(&mut self, actor: &mut A, ctx: &mut ActorContext);
}

struct ActorMessage<M, A>
where
    M: Message,
    A: Actor + Handler<M>,
{
    payload: M,
    rsvp: Option<oneshot::Sender<M::Response>>,
    _phantom_actor: PhantomData<A>,
}

#[async_trait]
impl<M, A> MessageHandler<A> for ActorMessage<M, A>
where
    M: Message,

    A: Actor + Handler<M>,
{
    async fn handle(&mut self, actor: &mut A, ctx: &mut ActorContext) {
        self.process(actor, ctx).await
    }
}

impl<M, A> ActorMessage<M, A>
where
    M: Message,

    A: Actor + Handler<M>,
{
    async fn process(&mut self, actor: &mut A, ctx: &mut ActorContext) {
        let result = actor.handle(self.payload.clone(), ctx).await;

        if let Some(rsvp) = std::mem::replace(&mut self.rsvp, None) {
            rsvp.send(result).unwrap_or_else(|_failed| {
                log::error!("Failed to send back response!");
            })
        }
    }

    pub fn new(msg: M, rsvp: Option<oneshot::Sender<M::Response>>) -> Self {
        ActorMessage {
            payload: msg,
            rsvp,
            _phantom_actor: PhantomData,
        }
    }
}

pub type MailboxReceiver<A> = mpsc::UnboundedReceiver<BoxedMessageHandler<A>>;
pub type MailboxSender<A> = mpsc::UnboundedSender<BoxedMessageHandler<A>>;

pub struct ActorMailbox<A: Actor> {
    _phantom_actor: PhantomData<A>,
}

impl<A: Actor> ActorMailbox<A> {
    pub fn create() -> (MailboxSender<A>, MailboxReceiver<A>) {
        mpsc::unbounded_channel()
    }
}

pub type BoxedMessageHandler<A> = Box<dyn MessageHandler<A>>;

pub struct HandlerRef<A: Actor> {
    sender: mpsc::UnboundedSender<BoxedMessageHandler<A>>,
}

impl<A: Actor> Clone for HandlerRef<A> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
        }
    }
}

impl<A: Actor> HandlerRef<A> {
    pub(crate) fn new(sender: mpsc::UnboundedSender<BoxedMessageHandler<A>>) -> Self {
        HandlerRef { sender }
    }

    pub fn tell<M>(&self, msg: M) -> Result<(), ActorError>
    where
        M: Message,
        A: Handler<M>,
    {
        let message = ActorMessage::<M, A>::new(msg, None);
        if let Err(error) = self.sender.send(Box::new(message)) {
            log::error!("Failed to tell message! {}", error.to_string());
            Err(ActorError::SendError(error.to_string()))
        } else {
            Ok(())
        }
    }

    pub async fn ask<M>(&self, msg: M) -> Result<M::Response, ActorError>
    where
        M: Message,
        A: Handler<M>,
    {
        let (response_sender, response_receiver) = oneshot::channel();
        let message = ActorMessage::<M, A>::new(msg, Some(response_sender));
        if let Err(error) = self.sender.send(Box::new(message)) {
            log::error!("Failed to ask message! {}", error.to_string());
            Err(ActorError::SendError(error.to_string()))
        } else {
            response_receiver
                .await
                .map_err(|error| ActorError::SendError(error.to_string()))
        }
    }

    pub fn is_closed(&self) -> bool {
        self.sender.is_closed()
    }
}

#[cfg(test)]
mod tests {

    use crate::{system::ActorSystem, ActorPath};

    use super::*;

    #[derive(Default, Clone)]
    struct MyActor {
        counter: usize,
    }

    #[derive(Debug, Clone)]
    struct MyMessage(String);

    impl Message for MyMessage {
        type Response = usize;
    }

    #[async_trait]
    impl Handler<MyMessage> for MyActor {
        async fn handle(&mut self, msg: MyMessage, _ctx: &mut ActorContext) -> usize {
            log::debug!("received message! {:?}", &msg);
            self.counter += 1;
            log::debug!("counter is now {}", &self.counter);
            self.counter
        }
    }

    impl Actor for MyActor {}

    #[tokio::test]
    async fn actor_tell() {
        if std::env::var("RUST_LOG").is_err() {
            std::env::set_var("RUST_LOG", "trace");
        }
        let _ = env_logger::builder().is_test(true).try_init();

        let mut actor = MyActor { counter: 0 };
        let msg = MyMessage("Hello World!".to_string());
        let (sender, mut receiver): (MailboxSender<MyActor>, MailboxReceiver<MyActor>) =
            ActorMailbox::create();
        let actor_ref = HandlerRef { sender };
        let system = ActorSystem::new("test");
        let path = ActorPath::from("/test");
        let mut ctx = ActorContext { path, system };
        tokio::spawn(async move {
            while let Some(mut msg) = receiver.recv().await {
                msg.handle(&mut actor, &mut ctx).await;
            }
        });

        actor_ref.tell(msg).unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
    }

    #[tokio::test]
    async fn actor_ask() {
        if std::env::var("RUST_LOG").is_err() {
            std::env::set_var("RUST_LOG", "trace");
        }
        let _ = env_logger::builder().is_test(true).try_init();

        let mut actor = MyActor { counter: 0 };
        let msg = MyMessage("Hello World!".to_string());
        let (sender, mut receiver): (MailboxSender<MyActor>, MailboxReceiver<MyActor>) =
            ActorMailbox::create();
        let actor_ref = HandlerRef { sender };
        let system = ActorSystem::new("test");
        let path = ActorPath::from("/test");
        let mut ctx = ActorContext { path, system };
        tokio::spawn(async move {
            while let Some(mut msg) = receiver.recv().await {
                msg.handle(&mut actor, &mut ctx).await;
            }
        });

        let result = actor_ref.ask(msg).await.unwrap();
        assert_eq!(result, 1);
    }
}
