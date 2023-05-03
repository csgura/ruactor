use std::{
    marker::PhantomData,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use crossbeam::queue::{ArrayQueue, SegQueue};
use rayon::ThreadPool;
use tokio::sync::Mutex;

use crate::{Actor, ActorPath, Props, PropsOption};

use super::{ActorRef, Dispatcher, InternalMessage, Message, ParentRef, PropWrap, Scheduler};

#[allow(dead_code)]
pub(crate) struct TokioChannelQueue<T: 'static + Send> {
    dedicated: bool,
    sender: tokio::sync::mpsc::UnboundedSender<Message<T>>,
    receiver: tokio::sync::mpsc::UnboundedReceiver<Message<T>>,
    num_msg: Arc<AtomicUsize>,
}

#[derive(Clone)]
enum Either<L: Clone, R: Clone> {
    Left(L),
    Right(R),
}

pub(crate) struct CrossbeamSegQueue<T: 'static + Send> {
    queue: Either<Arc<SegQueue<Message<T>>>, Arc<ArrayQueue<Message<T>>>>,
}

#[allow(dead_code)]
pub(crate) struct TokioChannelSender<T: 'static + Send> {
    sender: tokio::sync::mpsc::UnboundedSender<Message<T>>,
    num_msg: Arc<AtomicUsize>,
}

#[allow(dead_code)]
impl<T: 'static + Send> TokioChannelSender<T> {
    pub fn len(&self) -> usize {
        self.num_msg.load(Ordering::Acquire)
    }

    pub fn push(&self, msg: Message<T>) {
        let _ = self.sender.send(msg);
        self.num_msg.fetch_add(1, Ordering::Release);
    }
}
#[allow(dead_code)]
impl<T: 'static + Send> TokioChannelQueue<T> {
    pub fn new(dedicated: bool) -> Self {
        let ch = tokio::sync::mpsc::unbounded_channel();

        TokioChannelQueue {
            dedicated,
            sender: ch.0,
            receiver: ch.1,
            num_msg: Default::default(),
        }
    }

    pub fn sender(&self) -> TokioChannelSender<T> {
        return TokioChannelSender {
            sender: self.sender.clone(),
            num_msg: self.num_msg.clone(),
        };
    }
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.num_msg.load(Ordering::Acquire)
    }

    #[allow(dead_code)]
    pub fn push(&self, msg: Message<T>) {
        let _ = self.sender.send(msg);
        self.num_msg.fetch_add(1, Ordering::Release);
    }

    pub async fn pop(&mut self) -> Option<Message<T>> {
        let ret = if self.dedicated {
            self.receiver.try_recv().ok()
        } else {
            tokio::time::timeout(Duration::from_secs(1), self.receiver.recv())
                .await
                .unwrap_or(None)
        };
        if ret.is_some() {
            self.num_msg.fetch_sub(1, Ordering::Release);
        }
        ret
    }
}

impl<T: 'static + Send> CrossbeamSegQueue<T> {
    #[allow(unused_variables)]
    pub fn unbounded(dedicated: bool) -> Self {
        CrossbeamSegQueue {
            queue: Either::Left(Default::default()),
        }
    }

    #[allow(unused_variables)]
    pub fn bounded(dedicated: bool, size: usize) -> Self {
        CrossbeamSegQueue {
            queue: Either::Right(Arc::new(ArrayQueue::new(size))),
        }
    }

    pub fn sender(&self) -> CrossbeamSegQueue<T> {
        CrossbeamSegQueue {
            queue: self.queue.clone(),
        }
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        match &self.queue {
            Either::Left(l) => l.len(),
            Either::Right(r) => r.len(),
        }
    }

    #[allow(dead_code)]
    pub fn push(&self, msg: Message<T>) -> Result<(), Message<T>> {
        match &self.queue {
            Either::Left(l) => {
                l.push(msg);
                Ok(())
            }
            Either::Right(r) => r.push(msg),
        }
    }

    pub fn pop(&mut self) -> Option<Message<T>> {
        match &self.queue {
            Either::Left(l) => l.pop(),
            Either::Right(r) => r.pop(),
        }
    }
}

pub struct Mailbox<T: 'static + Send> {
    pub(crate) option: PropsOption,
    pub(crate) internal_queue: SegQueue<InternalMessage>,
    //pub(crate) message_queue: SegQueue<Message<T>>,
    pub(crate) message_queue: CrossbeamSegQueue<T>,

    pub(crate) running: AtomicBool,
    pub(crate) terminated: AtomicBool,
    pub(crate) dispatcher: Mutex<Dispatcher<T>>,
    pub(crate) handle: tokio::runtime::Handle,
    pub(crate) pool: Arc<ThreadPool>,
    pub(crate) dedicated_runtime: Option<tokio::runtime::Runtime>,
    pub(crate) child_runtime: Option<tokio::runtime::Runtime>,
    pub(crate) scheduler: Arc<Scheduler>,
}

impl<T: 'static + Send> Drop for Mailbox<T> {
    fn drop(&mut self) {
        let runtime = self.dedicated_runtime.take();
        if let Some(runtime) = runtime {
            self.pool.install(move || {
                drop(runtime);
            })
        }

        let runtime = self.child_runtime.take();
        if let Some(runtime) = runtime {
            self.pool.install(move || {
                drop(runtime);
            })
        }
    }
}

// impl<T: 'static + Send> Clone for Mailbox<T> {
//     fn clone(&self) -> Self {
//         Self {
//             internal_queue: SegQueue::new(),
//             message_queue: SegQueue::new(),
//             running: self.running.clone(),
//             terminated: self.terminated.clone(),
//             dispatcher: self.dispatcher.clone(),
//             handle: self.handle.clone(),
//         }
//     }
// }

pub(crate) async fn receive<T: 'static + Send>(self_ref: ActorRef<T>) {
    let mbox = self_ref.mbox.as_ref();

    if let Ok(mut dispatcher) = mbox.dispatcher.try_lock() {
        dispatcher.actor_loop(self_ref.clone()).await;
        drop(dispatcher);

        mbox.running.store(false, Ordering::Release);

        let num_msg = mbox.num_user_message();
        if num_msg > 0 && !mbox.is_terminated() {
            mbox.schedule(self_ref.clone());
        } else {
            let num_sys_msg = mbox.num_internal_message();
            if num_sys_msg > 0 {
                mbox.schedule(self_ref.clone());
            }
        }
    }
}

impl<T: 'static + Send> Mailbox<T> {
    pub(crate) fn new<P, A>(
        path: ActorPath,
        p: P,
        parent: Option<Box<dyn ParentRef>>,
        pool: Arc<ThreadPool>,
        handle: tokio::runtime::Handle,
        scheduler: Arc<Scheduler>,
    ) -> Mailbox<T>
    where
        P: Props<A>,
        A: Actor<Message = T>,
    {
        let option = p.option();
        let dedicated_thread = option.dedicated_thread();
        let pdyn = PropWrap {
            prop: p,
            phantom: PhantomData,
        };

        //let message_queue = TokioChannelQueue::new(dedicated_thread.is_some());
        let message_queue = if let Some(size) = option.bounded() {
            CrossbeamSegQueue::bounded(dedicated_thread.is_some(), size)
        } else {
            CrossbeamSegQueue::unbounded(dedicated_thread.is_some())
        };

        let message_sender = message_queue.sender();

        let dispatcher = Dispatcher {
            parent,
            actor: None,
            prop: Box::new(pdyn),
            last_message_timestamp: Instant::now(),
            context: None,
            message_queue,
            watcher: Default::default(),
        };

        let dedicated_runtime = dedicated_thread.and_then(|_| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .ok()
        });

        let (child_runtime, actor_handle) = if let Some(nworker) = dedicated_thread {
            let nworker = std::cmp::max(2, nworker);

            let runtime = tokio::runtime::Builder::new_multi_thread()
                .thread_name(path.to_string())
                .worker_threads(nworker)
                .enable_all()
                .build()
                .ok();
            let handle = runtime
                .as_ref()
                .map(|x| x.handle().clone())
                .unwrap_or(handle);
            (runtime, handle)
        } else {
            (None, handle)
        };

        let mbox = Mailbox {
            option,
            internal_queue: SegQueue::new(),
            dispatcher: Mutex::new(dispatcher),
            //message_queue: SegQueue::new(),
            message_queue: message_sender,
            running: false.into(),
            terminated: false.into(),
            handle: actor_handle,
            dedicated_runtime: dedicated_runtime,
            child_runtime,
            pool: pool.clone(),
            scheduler: scheduler,
        };
        mbox
    }

    //#[async_recursion::async_recursion]

    pub(crate) fn num_internal_message(&self) -> usize {
        self.internal_queue.len()
    }

    #[allow(dead_code)]
    pub(crate) fn num_total_message(&self) -> usize {
        self.internal_queue.len() + self.message_queue.len()
    }

    pub(crate) fn num_user_message(&self) -> usize {
        self.message_queue.len()
    }

    pub(crate) fn schedule(&self, self_ref: ActorRef<T>) {
        if let Ok(_) =
            self.running
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        {
            if let Some(runtime) = &self.dedicated_runtime {
                let handle = runtime.handle().clone();

                self.pool.spawn(move || {
                    let sc = self_ref.clone();
                    //let _guard = handle.enter();
                    handle.block_on(async move {
                        receive(sc).await;
                    });
                })
            } else {
                self.handle.spawn(async move { receive(self_ref).await });
            }
        }
    }

    pub(crate) fn is_terminated(&self) -> bool {
        self.terminated.load(Ordering::Acquire)
    }

    pub(crate) fn close(&self) {
        self.terminated.store(true, Ordering::Release);
    }

    pub(crate) fn send(&self, self_ref: ActorRef<T>, msg: Message<T>) {
        if !self.is_terminated() {
            if let Ok(_) = self.message_queue.push(msg) {
                self.schedule(self_ref);
            }
        }
    }

    pub(crate) fn try_send(
        &self,
        self_ref: ActorRef<T>,
        msg: Message<T>,
    ) -> Result<(), Message<T>> {
        if !self.is_terminated() {
            self.message_queue.push(msg)?;
            self.schedule(self_ref);
            Ok(())
        } else {
            Err(msg)
        }
    }

    pub(crate) fn send_internal(&self, self_ref: ActorRef<T>, msg: InternalMessage) {
        self.internal_queue.push(msg);
        self.schedule(self_ref);
    }
}
