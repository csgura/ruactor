use std::marker::PhantomData;
use std::ops::Deref;
use std::{collections::HashMap, sync::Arc};

use std::sync::{RwLock, Weak};

use rayon::ThreadPool;
use tokio::runtime::{Handle, RuntimeFlavor};

use crate::actor::{ChildContainer, InternalMessage, ParentRef, Scheduler};
use crate::ActorError;
use crate::{
    actor::{Actor, ActorRef, Mailbox},
    path::ActorPath,
};

/// Events that this actor system will send

#[derive(Clone, Copy)]
pub struct PropsOption {
    num_thread: Option<usize>,
    graceful_stop: bool,
    throughput: usize,
    bounded: Option<usize>,
}

impl Default for PropsOption {
    fn default() -> Self {
        Self {
            num_thread: Default::default(),
            graceful_stop: Default::default(),
            throughput: 500,
            bounded: None,
        }
    }
}

impl PropsOption {
    pub fn dedicated_thread(&self) -> Option<usize> {
        self.num_thread
    }

    pub fn graceful_stop(&self) -> bool {
        self.graceful_stop
    }

    pub fn throughput(&self) -> usize {
        self.throughput
    }

    pub fn bounded(&self) -> Option<usize> {
        self.bounded
    }
}

pub trait Props<A: Actor>: 'static + Send + Sized {
    fn create(&self) -> A;
    fn option(&self) -> PropsOption;
    fn with_dedicated_thread(self, num_thread: usize) -> PropsWithOption<A, Self> {
        PropsWithOption {
            option: PropsOption {
                num_thread: Some(num_thread),
                ..self.option()
            },
            phantom: PhantomData,
            props: self,
        }
    }
    fn with_graceful_stop(self) -> PropsWithOption<A, Self> {
        PropsWithOption {
            option: PropsOption {
                graceful_stop: true,
                ..self.option()
            },
            phantom: PhantomData,
            props: self,
        }
    }
    fn with_throughput(self, throughput: usize) -> PropsWithOption<A, Self> {
        PropsWithOption {
            option: PropsOption {
                throughput,
                ..self.option()
            },
            phantom: PhantomData,
            props: self,
        }
    }

    fn with_bounded_queue(self, size: usize) -> PropsWithOption<A, Self> {
        PropsWithOption {
            option: PropsOption {
                bounded: Some(size),
                ..self.option()
            },
            phantom: PhantomData,
            props: self,
        }
    }
}

pub struct PropsWithOption<A: Actor, P: Props<A> + 'static + Send> {
    props: P,
    option: PropsOption,
    phantom: PhantomData<A>,
}

impl<A: Actor, P: Props<A> + 'static + Send> Props<A> for PropsWithOption<A, P> {
    fn create(&self) -> A {
        self.props.create()
    }

    fn option(&self) -> PropsOption {
        self.option
    }
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

    fn option(&self) -> PropsOption {
        return PropsOption::default();
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

    fn option(&self) -> PropsOption {
        return PropsOption::default();
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
    fn option(&self) -> PropsOption;
}

struct UserGuard(RwLock<HashMap<ActorPath, ChildContainer>>);

impl Deref for UserGuard {
    type Target = RwLock<HashMap<ActorPath, ChildContainer>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Clone)]
pub struct ActorSystem {
    name: String,
    scheduler: Arc<Scheduler>,
    actors: Arc<UserGuard>,
    pool: Arc<ThreadPool>,
}

impl UserGuard {
    async fn stop_actor_wait(&self, path: &ActorPath) {
        let actor = {
            let mut actors = self.0.write().unwrap();
            actors.remove(path)
        };

        if let Some(actor) = actor {
            actor.stop_ref.wait_stop().await;
        }
    }

    fn stop_actor(&self, path: &ActorPath) {
        let actor = {
            let mut actors = self.0.write().unwrap();
            actors.remove(path)
        };

        if let Some(actor) = actor {
            actor.stop_ref.stop();
        }
    }
}
impl Drop for UserGuard {
    fn drop(&mut self) {
        let h = Handle::try_current();
        match h {
            Ok(h) if h.runtime_flavor() == RuntimeFlavor::MultiThread => {
                tokio::task::block_in_place(|| {
                    Handle::current().block_on(async move {
                        let keys = {
                            let actors = self.0.read().unwrap();

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
                    let actors = self.0.read().unwrap();

                    actors.keys().map(|v| v.clone()).collect::<Vec<_>>()
                };

                for k in keys {
                    self.stop_actor(&k);
                }
            }
        }
    }
}

struct RootActorStoper(Weak<UserGuard>);

impl ParentRef for RootActorStoper {
    fn send_internal_message(&self, message: crate::actor::InternalMessage) {
        match message {
            crate::actor::InternalMessage::ChildTerminate(path) => {
                let guard = self.0.upgrade();
                if let Some(guard) = guard {
                    let mut m = guard.write().unwrap();
                    let _removed = m.remove(&path);
                }
            }
            crate::actor::InternalMessage::Created => {}
            crate::actor::InternalMessage::Terminate => {}
            crate::actor::InternalMessage::Watch(_) => {}
        }
    }
}

impl ActorSystem {
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

        let parent = RootActorStoper(Arc::downgrade(&self.actors));

        let mut actors = self.actors.write().unwrap();
        if actors.contains_key(&path) {
            return Err(ActorError::Exists(path));
        }

        let mbox = Mailbox::new(
            path.clone(),
            actor,
            Some(Box::new(parent)),
            self.pool.clone(),
            tokio::runtime::Handle::current(),
            self.scheduler.sender(),
        );

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

        actor_ref.send_internal_message(InternalMessage::Created);

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
        let actors = RwLock::new(HashMap::new());

        let thread_name_prefix = name.clone();
        let pool = rayon::ThreadPoolBuilder::new()
            .thread_name(move |idx| format!("{}-{}", thread_name_prefix, idx))
            .num_threads(num_cpus::get())
            .build()
            .unwrap();

        let sced = Arc::new(Scheduler::new());

        sced.run(&name);

        ActorSystem {
            name,
            actors: Arc::new(UserGuard(actors)),
            pool: Arc::new(pool),
            scheduler: sced,
        }
    }
}
