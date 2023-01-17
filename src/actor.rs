use std::{
    collections::HashMap,
    mem::replace,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    },
    time::Duration,
};

use tokio::{sync::Mutex, time::Sleep};

use crate::{path::ActorPath, system::Prop};

pub struct ActorRef<T: 'static + Send> {
    mbox: Arc<Mailbox<T>>,
    path: ActorPath,
}

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

struct TimerMessage<T: 'static> {
    sleep: Sleep,
    msg: T,
    repeat: bool,
}

struct Timer<T: 'static> {
    list: HashMap<String, TimerMessage<T>>,
}

impl<T: 'static> Default for Timer<T> {
    fn default() -> Self {
        Self {
            list: Default::default(),
        }
    }
}

pub struct Context<T: 'static + Send> {
    self_ref: ActorRef<T>,
    actor: Option<Box<dyn Actor<UserMessageType = T>>>,
    stash: Option<T>,
    timer: Timer<T>,
}

impl<T: 'static + Send> Context<T> {
    pub fn transit<A: Actor<UserMessageType = T> + 'static>(&mut self, new_actor: A) {
        self.actor = Some(Box::new(new_actor));
    }

    pub fn start_single_timer(&mut self, d: Duration, t: T) {
        let self_ref = self.self_ref.clone();
        tokio::spawn(async move {
            let s = tokio::time::sleep(d);
            s.await;

            self_ref.send(Message::User(t));
        });
    }
}

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

pub struct ActorCell<T: 'static + Send> {
    actor: Box<dyn Actor<UserMessageType = T>>,
    ch: tokio::sync::mpsc::UnboundedReceiver<Message<T>>,
    stash: Vec<T>,
    timer: Timer<T>,
}

impl<T: 'static + Send> ActorCell<T> {
    fn with_context_transit(
        &mut self,
        self_ref: ActorRef<T>,
        a: &Box<dyn Actor<UserMessageType = T>>,
        f: fn(&mut Self, &Box<dyn Actor<UserMessageType = T>>, &mut Context<T>),
    ) {
        let dt = Timer::default();

        let mut context = Context {
            self_ref: self_ref.clone(),
            actor: None,
            stash: None,
            timer: replace(&mut self.timer, dt),
        };

        f(self, a, &mut context);

        self.timer = context.timer;

        if let Some(mess) = context.stash {
            self.stash.push(mess);
        }

        if let Some(new_actor) = context.actor {
            let old_actor = replace(&mut self.actor, new_actor);

            self.with_context_transit(self_ref.clone(), &old_actor, |cell, actor, ctx| {
                actor.on_exit(ctx)
            });

            self.with_context_transit(self_ref.clone(), &old_actor, |cell, actor, ctx| {
                cell.actor.on_enter(ctx)
            });
        }
    }

    fn with_context<F>(&mut self, self_ref: ActorRef<T>, f: F)
    where
        F: FnOnce(&mut Self, &mut Context<T>),
    {
        let dt = Timer::default();

        let mut context = Context {
            self_ref: self_ref.clone(),
            actor: None,
            stash: None,
            timer: replace(&mut self.timer, dt),
        };

        f(self, &mut context);

        self.timer = context.timer;

        if let Some(mess) = context.stash {
            self.stash.push(mess);
        }

        if let Some(new_actor) = context.actor {
            let old_actor = replace(&mut self.actor, new_actor);

            self.with_context_transit(self_ref.clone(), &old_actor, |cell, actor, ctx| {
                actor.on_exit(ctx)
            });

            self.with_context_transit(self_ref.clone(), &old_actor, |cell, actor, ctx| {
                cell.actor.on_enter(ctx)
            });
        }
    }

    async fn actor_loop(&mut self, self_ref: ActorRef<T>) {
        let num_msg = self_ref.mbox.num_msg.clone();

        // let mut actor = cell.actor;
        // let mut ch = cell.ch;

        // let mut stash = cell.stash;
        if num_msg.load(Ordering::SeqCst) == 0 {
            return;
        }

        while let Some(msg) = self.ch.recv().await {
            num_msg.fetch_sub(1, Ordering::SeqCst);

            self.with_context(self_ref.clone(), |cell, ctx| {
                cell.actor.on_message(ctx, msg);
            });

            if num_msg.load(Ordering::SeqCst) == 0 {
                break;
            }
        }
    }
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

impl<T: 'static + Send> Mailbox<T> {
    pub fn new<P, A>(p: P) -> Mailbox<T>
    where
        P: Prop<A>,
        A: Actor<UserMessageType = T>,
    {
        let ch = tokio::sync::mpsc::unbounded_channel::<Message<T>>();

        let cell = ActorCell {
            actor: Box::new(p.create()),
            ch: ch.1,
            stash: Vec::new(),
            timer: Timer {
                list: HashMap::new(),
            },
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
