use std::{collections::HashMap, sync::Arc};

use std::sync::RwLock;

use tokio::runtime::{Handle, RuntimeFlavor};

use crate::actor::{ChildContainer, ParentRef};
use crate::ActorError;
use crate::{
    actor::{Actor, ActorRef, Mailbox},
    path::ActorPath,
};

/// Events that this actor system will send

pub trait Props<A: Actor>: 'static + Send {
    fn create(&self) -> A;
}

pub struct PropFunc<A, F>(pub F)
where
    A: Actor,
    F: Fn() -> A + 'static + Send;

impl<A, F> Props<A> for PropFunc<A, F>
where
    A: Actor,
    F: Fn() -> A + 'static + Send,
{
    fn create(&self) -> A {
        self.0()
    }
}

pub fn props_from_func<A, F>(f: F) -> PropFunc<A, F>
where
    A: Actor,
    F: Fn() -> A + 'static + Send,
{
    PropFunc(f)
}

pub struct PropClone<A>(pub A)
where
    A: Actor + Clone;

impl<A> Props<A> for PropClone<A>
where
    A: Actor + Clone,
{
    fn create(&self) -> A {
        self.0.clone()
    }
}

pub fn props_from_clone<A>(a: A) -> PropClone<A>
where
    A: Actor + Clone,
{
    PropClone(a)
}

pub trait PropDyn<T: 'static + Send>: Send + 'static {
    fn create(&self) -> Box<dyn Actor<Message = T>>;
}

#[derive(Clone)]
pub struct ActorSystem {
    name: String,
    actors: Arc<RwLock<HashMap<ActorPath, ChildContainer>>>,
}

impl Drop for ActorSystem {
    fn drop(&mut self) {
        let actors = self.actors.clone();
        let h = Handle::try_current();
        match h {
            Ok(h) if h.runtime_flavor() == RuntimeFlavor::MultiThread => {
                tokio::task::block_in_place(|| {
                    Handle::current().block_on(async move {
                        let keys = {
                            let actors = actors.read().unwrap();

                            actors.keys().map(|v| v.clone()).collect::<Vec<_>>()
                        };

                        for k in keys {
                            let _ = self.stop_actor_wait(&k).await;
                        }
                    })
                });
            }
            _ => {
                let keys = {
                    let actors = actors.read().unwrap();

                    actors.keys().map(|v| v.clone()).collect::<Vec<_>>()
                };

                for k in keys {
                    self.stop_actor(&k);
                }
            }
        }
    }
}

struct RootActorStoper(Arc<RwLock<HashMap<ActorPath, ChildContainer>>>);

impl ParentRef for RootActorStoper {
    fn send_internal_message(&self, message: crate::actor::InternalMessage) {
        match message {
            crate::actor::InternalMessage::ChildTerminate(path) => {
                let mut m = self.0.write().unwrap();
                let _removed = m.remove(&path);
            }
        }
    }
}

impl ActorSystem {
    async fn stop_actor_wait(&self, path: &ActorPath) {
        let actor = {
            let mut actors = self.actors.write().unwrap();
            actors.remove(path)
        };

        if let Some(actor) = actor {
            let _ = actor.stop_ref.wait_stop().await;
        }
    }

    fn stop_actor(&self, path: &ActorPath) {
        let actor = {
            let mut actors = self.actors.write().unwrap();
            actors.remove(path)
        };

        if let Some(actor) = actor {
            let _ = actor.stop_ref.stop();
        }
    }

    /// The name given to this actor system
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Retrieves an actor running in this actor system. If actor does not exist, a None
    /// is returned instead.
    pub fn get_actor<M: Send>(&self, path: &ActorPath) -> Option<ActorRef<M>> {
        let actors = self.actors.read().unwrap();
        actors
            .get(path)
            .and_then(|any| any.actor_ref.downcast_ref::<ActorRef<M>>().cloned())
    }

    pub(crate) fn create_actor_path<A: Actor, P: Props<A> + Send + 'static>(
        &self,
        path: ActorPath,
        actor: P,
    ) -> Result<ActorRef<A::Message>, ActorError> {
        log::debug!("Creating actor '{}' on system '{}'...", &path, &self.name);

        let parent = RootActorStoper(self.actors.clone());

        let mut actors = self.actors.write().unwrap();
        if actors.contains_key(&path) {
            return Err(ActorError::Exists(path));
        }

        let mbox = Mailbox::new(actor, Some(Box::new(parent)));

        let actor_ref = ActorRef::new(path, Arc::new(mbox));

        let path = actor_ref.path().clone();
        let any = Box::new(actor_ref.clone());

        actors.insert(
            path,
            ChildContainer {
                actor_ref: any,
                stop_ref: Box::new(actor_ref.clone()),
            },
        );

        Ok(actor_ref)
    }

    /// Launches a new top level actor on this actor system at the '/user' actor path. If another actor with
    /// the same name already exists, an `Err(ActorError::Exists(ActorPath))` is returned instead.
    pub fn create_actor<A: Actor, P: Props<A> + Send + 'static>(
        &self,
        name: &str,
        actor: P,
    ) -> Result<ActorRef<A::Message>, ActorError> {
        let path = ActorPath::from("/user") / name;
        self.create_actor_path(path, actor)
    }

    /// Retrieve or create a new actor on this actor system if it does not exist yet.
    pub fn get_or_create_actor<A, F>(
        &self,
        name: &str,
        actor_fn: F,
    ) -> Result<ActorRef<A::Message>, ActorError>
    where
        A: Actor,
        F: Props<A> + Send + 'static,
    {
        let path = ActorPath::from("/user") / name;
        self.get_or_create_actor_path(&path, actor_fn)
    }

    pub(crate) fn get_or_create_actor_path<A, F>(
        &self,
        path: &ActorPath,
        actor_fn: F,
    ) -> Result<ActorRef<A::Message>, ActorError>
    where
        A: Actor,
        F: Props<A> + Send + 'static,
    {
        let actors = self.actors.read();
        match self.get_actor::<A::Message>(path) {
            Some(actor) => Ok(actor),
            None => {
                drop(actors);
                self.create_actor_path(path.clone(), actor_fn)
            }
        }
    }

    /// Stops the actor on this actor system. All its children will also be stopped.
    // pub fn stop_actor(&self, path: &ActorPath) {
    //     log::debug!("Stopping actor '{}' on system '{}'...", &path, &self.name);
    //     let mut paths: Vec<ActorPath> = vec![path.clone()];
    //     {
    //         let running_actors = self.actors.read().unwrap();
    //         for running in running_actors.keys() {
    //             if running.is_descendant_of(path) {
    //                 paths.push(running.clone());
    //             }
    //         }
    //     }
    //     paths.sort_unstable();
    //     paths.reverse();
    //     let mut actors = self.actors.write().unwrap();
    //     for path in &paths {
    //         actors.remove(path);
    //     }
    // }

    /// Creats a new actor system on which you can create actors.
    pub fn new(name: &str) -> Self {
        let name = name.to_string();
        let actors = Arc::new(RwLock::new(HashMap::new()));
        ActorSystem { name, actors }
    }
}
