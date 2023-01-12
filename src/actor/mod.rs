pub(crate) mod handler;
pub(crate) mod runner;
pub(crate) mod supervision;

use std::{any::Any, marker::PhantomData, sync::Arc};

use async_trait::async_trait;
use thiserror::Error;

mod path;
pub use path::ActorPath;

use supervision::SupervisionStrategy;

use crate::system::{ActorSystem, Prop};

// #[async_trait]
// pub trait ActorContext: Send + Sync + 'static {
//     type UserMessageType;

//     async fn create_child<A: Actor, P: Prop<A> + Send + 'static>(
//         &self,
//         name: &str,
//         actor: P,
//     ) -> Result<ActorRef<A::UserMessageType>, ActorError>;

//     async fn get_child<A: Actor>(&self, name: &str) -> Option<ActorRef<A::UserMessageType>>;

//     async fn get_or_create_child<A, F>(
//         &self,
//         name: &str,
//         actor_fn: F,
//     ) -> Result<ActorRef<A::UserMessageType>, ActorError>
//     where
//         A: Actor,
//         F: Prop<A> + Send + 'static;

//     async fn stop_child(&self, name: &str);

//     // async fn restart<A>(
//     //     &mut self,
//     //     actor: &mut A,
//     //     error: Option<&ActorError>,
//     // ) -> Result<(), ActorError>
//     // where
//     //     A: Actor;
// }

pub struct ReceiveMethod<T, F>
where
    F: Fn(&mut ActorContext<T>, Message<T>) + Sync + Send + 'static,
{
    f: F,
    phantom: PhantomData<T>,
}

pub fn create_receive<T, F>(f: F) -> Box<dyn Receive<T>>
where
    T: Send + Sync + 'static,
    F: Fn(&mut ActorContext<T>, Message<T>) + Sync + Send + 'static,
{
    Box::new(ReceiveMethod {
        f: f,
        phantom: PhantomData {},
    })
}

#[async_trait]
pub trait Receive<T: Send + Sync>: 'static + Send + Sync {
    async fn receive(&self, ctx: &mut ActorContext<T>, msg: Message<T>);
}

#[async_trait]
impl<T: Send + Sync + 'static, F> Receive<T> for ReceiveMethod<T, F>
where
    F: Fn(&mut ActorContext<T>, Message<T>) + Sync + Send + 'static,
{
    async fn receive(&self, ctx: &mut ActorContext<T>, msg: Message<T>) {
        (self.f)(ctx, msg)
    }
}

/// The actor context gives a running actor access to its path, as well as the system that
/// is running it.
pub struct ActorContext<T> {
    pub path: ActorPath,
    pub system: ActorSystem,
    receiver: Arc<dyn Receive<T>>,
}

impl<T: 'static> ActorContext<T> {
    /// Create a child actor under this actor.
    pub async fn create_child<A: Actor, P: Prop<A> + Send + 'static>(
        &self,
        name: &str,
        actor: P,
    ) -> Result<ActorRef<A::UserMessageType>, ActorError> {
        let path = self.path.clone() / name;
        self.system.create_actor_path(path, actor).await
    }

    /// Retrieve a child actor running under this actor.
    pub async fn get_child<A: Actor>(&self, name: &str) -> Option<ActorRef<A::UserMessageType>> {
        let path = self.path.clone() / name;
        self.system.get_actor::<A>(&path).await
    }

    /// Retrieve or create a new child under this actor if it does not exist yet
    pub async fn get_or_create_child<A, F>(
        &self,
        name: &str,
        actor_fn: F,
    ) -> Result<ActorRef<A::UserMessageType>, ActorError>
    where
        A: Actor,
        F: Prop<A> + Send + 'static,
    {
        let path = self.path.clone() / name;
        self.system.get_or_create_actor_path(&path, actor_fn).await
    }

    /// Stops the child actor
    pub async fn stop_child(&self, name: &str) {
        let path = self.path.clone() / name;
        self.system.stop_actor(&path).await;
    }

    // pub(crate) async fn restart<A>(
    //     &mut self,
    //     actor: &mut A,
    //     error: Option<&ActorError>,
    // ) -> Result<(), ActorError>
    // where
    //     A: Actor,
    // {
    //     actor.pre_restart(self, error).await
    // }
}

/// Defines what an actor will receive as its message, and with what it should respond.
pub trait Request: Clone + Send + Sync + 'static {
    /// response an actor should give when it receives this message. If no response is
    /// required, use `()`.
    type Response: Send + Sync + 'static;
}

/// Defines what the actor does with a message.
#[async_trait]
pub trait Handler<M: Request>: Send + Sync {
    async fn handle(&mut self, msg: M, ctx: &mut ActorContext<M>) -> M::Response;
}

#[derive(Debug)]
pub enum Message<M: Send + Sync> {
    SystemMessage,
    UserMessage(M),
}

#[async_trait]
pub trait Actor: Send + Sync + 'static {
    type UserMessageType: Send + Sync;

    fn create_receive(&mut self) -> Box<dyn Receive<Self::UserMessageType>> {
        todo!();
    }

    /// Defines the supervision strategy to use for this actor. By default it is
    /// `Stop` which simply stops the actor if an error occurs at startup. You
    /// can also set this to [`SupervisionStrategy::Retry`] with a chosen
    /// [`supervision::RetryStrategy`].
    fn supervision_strategy() -> SupervisionStrategy {
        SupervisionStrategy::Stop
    }

    /// Override this function if you like to perform initialization of the actor
    async fn pre_start(
        &mut self,
        _ctx: &mut ActorContext<Self::UserMessageType>,
    ) -> Result<(), ActorError> {
        Ok(())
    }

    /// Override this function if you want to define what should happen when an
    /// error occurs in [`Actor::pre_start()`]. By default it simply calls
    /// `pre_start()` again, but you can also choose to reinitialize the actor
    /// in some other way.
    async fn pre_restart(
        &mut self,
        ctx: &mut ActorContext<Self::UserMessageType>,
        _error: Option<&ActorError>,
    ) -> Result<(), ActorError> {
        self.pre_start(ctx).await
    }

    /// Override this function if you like to perform work when the actor is stopped
    async fn post_stop(&mut self, _ctx: &mut ActorContext<Self::UserMessageType>) {}
}

/// A clonable actor reference. It basically holds a Sender that can send messages
/// to the mailbox (receiver) of the actor.
pub struct ActorRef<T: Send + Sync> {
    path: ActorPath,
    sender: handler::HandlerRef<T>,
}

impl<A> Clone for ActorRef<A>
where
    A: Send + Sync,
{
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            sender: self.sender.clone(),
        }
    }
}

impl<T: Send + Sync> ActorRef<T> {
    /// Get the path of this actor
    pub fn path(&self) -> &ActorPath {
        &self.path
    }

    /// Get the path of this actor
    #[deprecated(since = "0.2.3", note = "please use `path` instead")]
    pub fn get_path(&self) -> &ActorPath {
        &self.path
    }

    /// Fire and forget sending of messages to this actor.
    pub fn tell(&self, msg: Message<T>) -> Result<(), ActorError> {
        self.sender.tell(msg)
    }

    /// Checks if the actor message box is still open. If it is closed, the actor
    /// is not running.
    pub fn is_closed(&self) -> bool {
        self.sender.is_closed()
    }

    pub(crate) fn new(path: ActorPath, sender: handler::MailboxSender<T>) -> Self {
        let handler = handler::HandlerRef::new(sender);
        ActorRef {
            path,
            sender: handler,
        }
    }
}

impl<A> std::fmt::Debug for ActorRef<A>
where
    A: Copy + Send + Sync + Clone,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.path)
    }
}

#[derive(Error, Debug)]
pub enum ActorError {
    #[error("Actor exists")]
    Exists(ActorPath),

    #[error("Actor creation failed")]
    CreateError(String),

    #[error("Sending message failed")]
    SendError(String),

    #[error("Actor runtime error")]
    RuntimeError(anyhow::Error),
}

impl ActorError {
    pub fn new<E>(error: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::RuntimeError(anyhow::Error::new(error))
    }
}
