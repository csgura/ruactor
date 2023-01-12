use std::marker::PhantomData;

use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};

use crate::{
    actor::{ActorContext, Handler, Request},
    ActorRef,
};

use super::{Actor, ActorError, Message};

pub type MailboxReceiver<T> = mpsc::UnboundedReceiver<Message<T>>;
pub type MailboxSender<T> = mpsc::UnboundedSender<Message<T>>;

pub struct ActorMailbox<A> {
    _phantom_actor: PhantomData<A>,
}

impl<T: Send + Sync> ActorMailbox<T> {
    pub fn create() -> (MailboxSender<T>, MailboxReceiver<T>) {
        mpsc::unbounded_channel()
    }
}

pub struct HandlerRef<T: Send + Sync> {
    sender: mpsc::UnboundedSender<Message<T>>,
}

impl<T> Clone for HandlerRef<T>
where
    T: Send + Sync,
{
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
        }
    }
}

impl<T> HandlerRef<T>
where
    T: Send + Sync,
{
    pub(crate) fn new(sender: mpsc::UnboundedSender<Message<T>>) -> Self {
        HandlerRef { sender }
    }

    pub fn tell(&self, msg: Message<T>) -> Result<(), ActorError> {
        //let message = ActorMessage::<M, A>::new(msg, None);
        if let Err(error) = self.sender.send(msg) {
            log::error!("Failed to tell message! {}", error.to_string());
            Err(ActorError::SendError(error.to_string()))
        } else {
            Ok(())
        }
    }

    // pub async fn ask<Res: Copy + Send + Sync>(&self, msg: T) -> Result<Res, ActorError> {
    //     let (response_sender, response_receiver) = mpsc::unbounded_channel::<Message<Res>>();

    //     let sender = ActorRef {
    //         path: "asker".into(),
    //         sender: HandlerRef {
    //             sender: response_sender,
    //         },
    //     };
    //     if let Err(error) = self.sender.send(message) {
    //         log::error!("Failed to ask message! {}", error.to_string());
    //         Err(ActorError::SendError(error.to_string()))
    //     } else {
    //         response_receiver
    //             .await
    //             .map_err(|error| ActorError::SendError(error.to_string()))
    //     }
    // }

    pub fn is_closed(&self) -> bool {
        self.sender.is_closed()
    }
}

#[cfg(test)]
mod tests {

    use crate::{actor::ActorContext, system::ActorSystem, ActorPath};

    use super::*;

    #[derive(Default, Clone)]
    struct MyActor {
        counter: usize,
    }

    #[derive(Debug, Clone)]
    struct MyMessage(String);

    impl Request for MyMessage {
        type Response = usize;
    }

    // #[async_trait]
    // impl Handler<MyMessage> for MyActor {
    //     async fn handle(&mut self, msg: MyMessage, _ctx: &mut ActorContext) -> usize {
    //         log::debug!("received message! {:?}", &msg);
    //         self.counter += 1;
    //         log::debug!("counter is now {}", &self.counter);
    //         self.counter
    //     }
    // }

    impl Actor for MyActor {
        type UserMessageType = MyMessage;
    }

    #[tokio::test]
    async fn actor_tell() {
        if std::env::var("RUST_LOG").is_err() {
            std::env::set_var("RUST_LOG", "trace");
        }
        let _ = env_logger::builder().is_test(true).try_init();

        let mut actor = MyActor { counter: 0 };
        let msg = MyMessage("Hello World!".to_string());
        let (sender, mut receiver): (MailboxSender<MyMessage>, MailboxReceiver<MyMessage>) =
            ActorMailbox::create();
        let actor_ref = HandlerRef { sender };
        let system = ActorSystem::new("test");
        let path = ActorPath::from("/test");
        let mut ctx = ActorContext {
            path,
            system,
            receiver: actor.create_receive(),
        };
        // tokio::spawn(async move {
        //     while let Some(mut msg) = receiver.recv().await {
        //         msg.handle(&mut actor, &mut ctx).await;
        //     }
        // });

        actor_ref.tell(Message::UserMessage(msg)).unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
    }
}
