use std::{
    collections::HashMap,
    marker::PhantomData,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    },
};

use tokio::sync::Mutex;

use crate::{Actor, Message, Prop};

use super::{ActorCell, ActorRef, PropWrap, Timer};

pub struct Mailbox<T: 'static + Send> {
    pub(crate) ch: tokio::sync::mpsc::UnboundedSender<Message<T>>,
    pub(crate) num_msg: Arc<AtomicU32>,
    pub(crate) status: Arc<AtomicBool>,
    pub(crate) cell: Arc<Mutex<ActorCell<T>>>,
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
            childrens: HashMap::new(),
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
            //println!("start receive loop");

            owned = true;
            self.status.store(true, Ordering::SeqCst);
            cell.actor_loop(self_ref.clone()).await;
            self.status.store(false, Ordering::SeqCst);
            //println!("end receive loop");
        } else {
            //println!("lock failed");
        }

        if owned {
            let num_msg = self.num_msg.load(Ordering::SeqCst);
            if num_msg > 0 {
                //println!("num msg = {}", num_msg);
                //self.receive().await;
                self.schedule(self_ref.clone());
            }
        }
    }

    pub fn schedule(&self, self_ref: ActorRef<T>) {
        if !self.status.load(Ordering::SeqCst) {
            //println!("schedule");
            let mut cl: Mailbox<T> = self.clone();

            tokio::spawn(async move {
                cl.receive(self_ref.clone()).await
                //cl.status.store(true, std::sync::atomic::Ordering::SeqCst);
                //actor_loop( &mut cell ).await;
            });
        } else {
            //println!("not schedule");
        }
    }

    pub fn send(&self, self_ref: ActorRef<T>, msg: Message<T>) {
        let ch = self.ch.clone();
        ch.send(msg);

        self.num_msg.fetch_add(1, Ordering::SeqCst);
        self.schedule(self_ref);
    }
}
