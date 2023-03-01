use std::{
    any::Any,
    fmt::{Debug, Display},
    marker::PhantomData,
    sync::Arc,
    time::Instant,
};

mod context;
mod dispatcher;
mod mailbox;

use crate::{
    path::ActorPath,
    system::{PropDyn, Props},
    ActorError, ReplyTo,
};

#[async_trait]
pub(crate) trait InternalActorRef: 'static + Send + Sync {
    fn stop(&self);
    async fn wait_stop(&self) -> Result<(), ActorError>;
}

pub struct ActorRef<T: 'static + Send> {
    mbox: Arc<Mailbox<T>>,
    path: ActorPath,
}

#[async_trait]
impl<T: 'static + Send> InternalActorRef for ActorRef<T> {
    fn stop(&self) {
        self.send(Message::Terminate(None));
    }

    async fn wait_stop(&self) -> Result<(), ActorError> {
        let ch = tokio::sync::oneshot::channel();

        self.send(Message::Terminate(Some(ch.0)));

        let ret = ch.1.await?;
        Ok(ret)
    }
}

pub(crate) trait ParentRef: 'static + Send {
    fn send_internal_message(&self, message: InternalMessage);
}

impl<T: 'static + Send> ParentRef for ActorRef<T> {
    fn send_internal_message(&self, message: InternalMessage) {
        self.mbox.send_internal(self.clone(), message)
    }
}

impl<T: 'static + Send> Display for ActorRef<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.path())
    }
}

impl<T: 'static + Send> Debug for ActorRef<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActorRef")
            .field("path", &self.path)
            .finish()
    }
}

use async_trait::async_trait;
pub use dispatcher::Dispatcher;
pub use mailbox::Mailbox;

impl<T: 'static + Send> ActorRef<T> {
    pub fn new(path: ActorPath, mbox: Arc<Mailbox<T>>) -> ActorRef<T> {
        ActorRef {
            mbox: mbox,
            path: path,
        }
    }

    pub fn path(&self) -> &ActorPath {
        &self.path
    }
}

impl<T: 'static + Send> Clone for ActorRef<T> {
    fn clone(&self) -> Self {
        Self {
            mbox: self.mbox.clone(),
            path: self.path.clone(),
        }
    }
}

impl<T: 'static + Send> ActorRef<T> {
    pub fn tell(&self, msg: T) {
        self.mbox.send(self.clone(), Message::User(msg));
    }

    pub(crate) fn send(&self, msg: Message<T>) {
        self.mbox.send(self.clone(), msg);
    }
}

pub use context::ActorContext;
pub use dispatcher::Timer;

#[derive(Debug)]
pub enum SystemMessage {
    ReceiveTimeout,
}

#[derive(Debug)]
pub(crate) enum Message<T: 'static + Send> {
    User(T),
    Timer(String, u32, T),
    ReceiveTimeout(Instant),
    Terminate(Option<ReplyTo<()>>),
}

#[derive(Debug)]
pub(crate) enum InternalMessage {
    ChildTerminate(ActorPath),
}

#[allow(unused_variables)]
#[async_trait]
pub trait Actor: Send + 'static {
    type Message: 'static + Send;

    async fn on_message_async(
        &mut self,
        context: &mut ActorContext<Self::Message>,
        message: Self::Message,
    ) {
        self.on_message(context, message)
    }

    fn on_enter(&mut self, context: &mut ActorContext<Self::Message>) {}

    fn on_exit(&mut self, context: &mut ActorContext<Self::Message>) {}

    fn on_message(&mut self, context: &mut ActorContext<Self::Message>, message: Self::Message);

    fn on_system_message(
        &mut self,
        context: &mut ActorContext<Self::Message>,
        message: SystemMessage,
    ) {
    }
}

struct PropWrap<A: Actor, P: Props<A>> {
    prop: P,
    phantom: PhantomData<A>,
}

impl<A: Actor, P: Props<A>> PropDyn<A::Message> for PropWrap<A, P> {
    fn create(&self) -> Box<dyn Actor<Message = A::Message>> {
        Box::new(self.prop.create())
    }
}

pub(crate) struct ChildContainer {
    actor_ref: Box<dyn Any + Send + Sync + 'static>,
    stop_ref: Box<dyn InternalActorRef>,
}
