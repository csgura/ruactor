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
    system::{Prop, PropDyn},
};

pub(crate) trait InternalActorRef: 'static + Send {
    fn stop(&self);
}

pub struct ActorRef<T: 'static + Send> {
    mbox: Arc<Mailbox<T>>,
    path: ActorPath,
}

impl<T: 'static + Send> InternalActorRef for ActorRef<T> {
    fn stop(&self) {
        self.send(Message::Terminate);
    }
}

pub(crate) trait ParentRef: 'static + Send {
    fn send_internal_message(&self, message: InternalMessage);
}

impl<T: 'static + Send> ParentRef for ActorRef<T> {
    fn send_internal_message(&self, message: InternalMessage) {
        self.send(Message::Internal(message))
    }
}

impl<T: 'static + Send> Display for ActorRef<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.path())
    }
}

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
    System(SystemMessage),
    User(T),
    Timer(String, u32, T),
    ReceiveTimeout(Instant),
    Terminate,
    Internal(InternalMessage),
}

#[derive(Debug)]
pub(crate) enum InternalMessage {
    ChildTerminate(ActorPath),
}

#[allow(unused_variables)]
pub trait Actor: Send + 'static {
    type Message: 'static + Send;

    fn on_enter(&mut self, context: &mut ActorContext<Self::Message>) {}

    fn on_exit(&mut self, context: &mut ActorContext<Self::Message>) {}

    fn on_message(&mut self, context: &mut ActorContext<Self::Message>, message: Self::Message);

    fn on_system_message(
        &mut self,
        _context: &mut ActorContext<Self::Message>,
        _message: SystemMessage,
    ) {
    }
}

struct PropWrap<A: Actor, P: Prop<A>> {
    prop: P,
    phantom: PhantomData<A>,
}

impl<A: Actor, P: Prop<A>> PropDyn<A::Message> for PropWrap<A, P> {
    fn create(&self) -> Box<dyn Actor<Message = A::Message>> {
        Box::new(self.prop.create())
    }
}

pub(crate) struct ChildContainer {
    actor_ref: Box<dyn Any + Send + Sync + 'static>,
    stop_ref: Box<dyn InternalActorRef>,
}
