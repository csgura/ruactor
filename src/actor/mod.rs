use std::{
    collections::HashMap,
    marker::PhantomData,
    mem::replace,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    },
    time::Duration,
};

mod cell;
mod context;

use tokio::{sync::Mutex, time::Sleep};

use crate::{
    path::ActorPath,
    system::{Prop, PropDyn},
};

pub struct ActorRef<T: 'static + Send> {
    mbox: Arc<Mailbox<T>>,
    path: ActorPath,
}

pub use cell::ActorCell;

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
    pub fn send(&self, msg: Message<T>) {
        self.mbox.send(self.clone(), msg);
    }
}

pub use cell::Timer;
pub use context::Context;

#[derive(Debug)]
pub enum SystemMessage {
    ReceiveTimeout,
}

#[derive(Debug)]
pub enum Message<T: 'static + Send> {
    System(SystemMessage),
    User(T),
}

pub trait Actor: Send + 'static {
    type UserMessageType: 'static + Send;

    fn on_enter(&self, context: &mut Context<Self::UserMessageType>) {
        println!("default on enter");
    }

    fn on_exit(&self, context: &mut Context<Self::UserMessageType>) {}

    fn on_message(
        &self,
        context: &mut Context<Self::UserMessageType>,
        message: Message<Self::UserMessageType>,
    );
}

pub struct Mailbox<T: 'static + Send> {
    ch: tokio::sync::mpsc::UnboundedSender<Message<T>>,
    num_msg: Arc<AtomicU32>,
    status: Arc<AtomicBool>,
    cell: Arc<Mutex<ActorCell<T>>>,
}

impl<T: 'static + Send> Clone for Mailbox<T> {
    fn clone(&self) -> Self {
        Self {
            ch: self.ch.clone(),
            num_msg: self.num_msg.clone(),
            status: self.status.clone(),
            cell: self.cell.clone(),
        }
    }
}

struct PropWrap<A: Actor, P: Prop<A>> {
    prop: P,
    phantom: PhantomData<A>,
}

impl<A: Actor, P: Prop<A>> PropDyn<A::UserMessageType> for PropWrap<A, P> {
    fn create(&self) -> Box<dyn Actor<UserMessageType = A::UserMessageType>> {
        Box::new(self.prop.create())
    }
}

impl<T: 'static + Send> Mailbox<T> {
    pub fn new<P, A>(p: P) -> Mailbox<T>
    where
        P: Prop<A>,
        A: Actor<UserMessageType = T>,
    {
        let pdyn = PropWrap {
            prop: p,
            phantom: PhantomData,
        };

        let ch = tokio::sync::mpsc::unbounded_channel::<Message<T>>();

        let cell = ActorCell {
            actor: None,
            ch: ch.1,
            stash: Vec::new(),
            timer: Timer {
                list: HashMap::new(),
            },
            prop: Box::new(pdyn),
            receive_timeout: None,
        };

        let mbox = Mailbox {
            cell: Arc::new(Mutex::new(cell)),
            ch: ch.0,
            num_msg: Arc::new(0.into()),
            status: Arc::new(false.into()),
        };
        mbox
    }

    //#[async_recursion::async_recursion]
    pub async fn receive(&self, self_ref: ActorRef<T>) {
        let mut owned = false;

        if let Ok(mut cell) = self.cell.try_lock() {
            println!("start receive loop");

            owned = true;
            self.status.store(true, Ordering::SeqCst);
            cell.actor_loop(self_ref.clone()).await;
            self.status.store(false, Ordering::SeqCst);
            println!("end receive loop");
        } else {
            println!("lock failed");
        }

        if owned {
            let num_msg = self.num_msg.load(Ordering::SeqCst);
            if num_msg > 0 {
                println!("num msg = {}", num_msg);
                //self.receive().await;
                self.schedule(self_ref.clone());
            }
        }
    }

    pub fn schedule(&self, self_ref: ActorRef<T>) {
        if !self.status.load(Ordering::SeqCst) {
            println!("schedule");
            let mut cl: Mailbox<T> = self.clone();

            tokio::spawn(async move {
                cl.receive(self_ref.clone()).await
                //cl.status.store(true, std::sync::atomic::Ordering::SeqCst);
                //actor_loop( &mut cell ).await;
            });
        } else {
            println!("not schedule");
        }
    }

    pub fn send(&self, self_ref: ActorRef<T>, msg: Message<T>) {
        let ch = self.ch.clone();
        ch.send(msg);

        self.num_msg.fetch_add(1, Ordering::SeqCst);
        self.schedule(self_ref);
    }
}
