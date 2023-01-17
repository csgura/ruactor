use std::{any::Any, collections::HashMap, marker::PhantomData, sync::Arc};

use tokio::sync::RwLock;

use thiserror::Error;

use crate::{
    actor::{Actor, ActorRef, Mailbox},
    path::ActorPath,
};

/// Events that this actor system will send

pub trait Prop<A: Actor>: 'static + Send {
    fn create(&self) -> A;
}

impl<A, F> Prop<A> for F
where
    A: Actor,
    F: Fn() -> A + 'static + Send,
{
    fn create(&self) -> A {
        self()
    }
}

pub trait PropDyn<T: 'static + Send>: Send + 'static {
    fn create(&self) -> Box<dyn Actor<UserMessageType = T>>;
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

#[derive(Clone)]
pub struct ActorSystem {
    name: String,
    actors: Arc<RwLock<HashMap<ActorPath, Box<dyn Any + Send + Sync + 'static>>>>,
}

impl ActorSystem {
    /// The name given to this actor system
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Retrieves an actor running in this actor system. If actor does not exist, a None
    /// is returned instead.
    pub async fn get_actor<A: Actor>(
        &self,
        path: &ActorPath,
    ) -> Option<ActorRef<A::UserMessageType>> {
        let actors = self.actors.read().await;
        actors
            .get(path)
            .and_then(|any| any.downcast_ref::<ActorRef<A::UserMessageType>>().cloned())
    }

    pub(crate) async fn create_actor_path<A: Actor, P: Prop<A> + Send + 'static>(
        &self,
        path: ActorPath,
        actor: P,
    ) -> Result<ActorRef<A::UserMessageType>, ActorError> {
        log::debug!("Creating actor '{}' on system '{}'...", &path, &self.name);

        let mut actors = self.actors.write().await;
        if actors.contains_key(&path) {
            return Err(ActorError::Exists(path));
        }

        let system = self.clone();
        let mbox = Mailbox::new(actor);

        let actor_ref = ActorRef::new(path, Arc::new(mbox));

        let path = actor_ref.path().clone();
        let any = Box::new(actor_ref.clone());

        actors.insert(path, any);

        Ok(actor_ref)
    }

    /// Launches a new top level actor on this actor system at the '/user' actor path. If another actor with
    /// the same name already exists, an `Err(ActorError::Exists(ActorPath))` is returned instead.
    pub async fn create_actor<A: Actor, P: Prop<A> + Send + 'static>(
        &self,
        name: &str,
        actor: P,
    ) -> Result<ActorRef<A::UserMessageType>, ActorError> {
        let path = ActorPath::from("/user") / name;
        self.create_actor_path(path, actor).await
    }

    /// Retrieve or create a new actor on this actor system if it does not exist yet.
    pub async fn get_or_create_actor<A, F>(
        &self,
        name: &str,
        actor_fn: F,
    ) -> Result<ActorRef<A::UserMessageType>, ActorError>
    where
        A: Actor,
        F: Prop<A> + Send + 'static,
    {
        let path = ActorPath::from("/user") / name;
        self.get_or_create_actor_path(&path, actor_fn).await
    }

    pub(crate) async fn get_or_create_actor_path<A, F>(
        &self,
        path: &ActorPath,
        actor_fn: F,
    ) -> Result<ActorRef<A::UserMessageType>, ActorError>
    where
        A: Actor,
        F: Prop<A> + Send + 'static,
    {
        let actors = self.actors.read().await;
        match self.get_actor::<A>(path).await {
            Some(actor) => Ok(actor),
            None => {
                drop(actors);
                self.create_actor_path(path.clone(), actor_fn).await
            }
        }
    }

    /// Stops the actor on this actor system. All its children will also be stopped.
    pub async fn stop_actor(&self, path: &ActorPath) {
        log::debug!("Stopping actor '{}' on system '{}'...", &path, &self.name);
        let mut paths: Vec<ActorPath> = vec![path.clone()];
        {
            let running_actors = self.actors.read().await;
            for running in running_actors.keys() {
                if running.is_descendant_of(path) {
                    paths.push(running.clone());
                }
            }
        }
        paths.sort_unstable();
        paths.reverse();
        let mut actors = self.actors.write().await;
        for path in &paths {
            actors.remove(path);
        }
    }

    /// Creats a new actor system on which you can create actors.
    pub fn new(name: &str) -> Self {
        let name = name.to_string();
        let actors = Arc::new(RwLock::new(HashMap::new()));
        ActorSystem { name, actors }
    }
}
