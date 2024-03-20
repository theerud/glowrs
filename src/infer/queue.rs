use anyhow::Result;
use std::marker::PhantomData;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::oneshot;
use tokio::time::Instant;
use uuid::Uuid;

use super::{TaskId, TaskRequest, TaskResponse};

/// Trait representing a stateful task processor
pub trait RequestHandler<TReq, TResp>
where
    TReq: TaskRequest,
    TResp: TaskResponse,
    Self: Sized
{
    // TODO: Optional initialization args?
    fn new() -> Result<Self>;
    fn handle(&mut self, request: TReq) -> Result<TResp>;
}

/// Queue entry
#[derive(Debug)]
pub(crate) struct QueueEntry<TReq, TResp>
where
    TReq: TaskRequest,
    TResp: TaskResponse,
{
    /// Identifier
    pub id: TaskId,

    /// Request
    pub request: TReq,

    /// Response sender
    pub response_tx: oneshot::Sender<TResp>,

    /// Instant when this entry was queued
    pub queue_time: Instant,
}


/// Queue entry
impl<TReq: TaskRequest, TResp: TaskResponse> QueueEntry<TReq, TResp> {
    pub fn new(request: TReq, response_tx: oneshot::Sender<TResp>) -> Self {
        Self {
            id: Uuid::new_v4(),
            request,
            response_tx,
            queue_time: Instant::now(),
        }
    }
}

/// Queue command
#[derive(Debug)]
// TODO: Use stop command
#[allow(dead_code)]
pub(crate) enum QueueCommand<TReq: TaskRequest, TResp: TaskResponse>
where
    TReq: Send,
{
    Append(QueueEntry<TReq, TResp>),
    Stop,
}

/// Request Queue with stateful task processor
#[derive(Clone)]
pub struct Queue<TReq, TResp, TProc>
where
    TReq: TaskRequest,
    TResp: TaskResponse,
    TProc: RequestHandler<TReq, TResp>,
{
    tx: UnboundedSender<QueueCommand<TReq, TResp>>,
    _processor: PhantomData<TProc>,
}

impl<TReq, TResp, TProc> Queue<TReq, TResp, TProc>
where
    TReq: TaskRequest,
    TResp: TaskResponse,
    TProc: RequestHandler<TReq, TResp> + 'static,
{
    pub(crate) fn new() -> Result<Self> {
        
        // TODO: Replace with MPMC w/ more worker threads (if CPU)
        // Create channel
        let (queue_tx, queue_rx) = unbounded_channel();
        
        let _join_handle = std::thread::spawn(move || {
            // Create task processor
            let processor = TProc::new()?;
            
            // Create a new Runtime to run tasks
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_name(format!("queue-{}", Uuid::new_v4()))
                // TODO: Make configurable
                .worker_threads(4)
                .build()?;

            // Pull task requests off the channel and send them to the executor
            runtime.block_on(queue_task(queue_rx, processor))
        });

        Ok(Self {
            tx: queue_tx,
            _processor: PhantomData,
        })
    }

    pub(crate) fn get_tx(&self) -> UnboundedSender<QueueCommand<TReq, TResp>> {
        self.tx.clone()
    }
}

// Generic background task executor with stateful processor
async fn queue_task<TReq, TResp, TProc>(
    mut receiver: UnboundedReceiver<QueueCommand<TReq, TResp>>,
    mut processor: TProc,
) -> Result<()>
where
    TReq: TaskRequest,
    TResp: TaskResponse,
    TProc: RequestHandler<TReq, TResp> + 'static,
{
    'main: while let Some(cmd) = receiver.recv().await {
        use QueueCommand::*;
        
        match cmd {
            Append(entry) => {
                tracing::trace!(
                    "Processing task {}, added {}ms ago",
                    entry.id,
                    entry.queue_time.elapsed().as_millis()
                );
                
                // Process the task 
                let response = processor.handle(entry.request)?;
                
                if entry.response_tx.send(response).is_ok() {
                    tracing::trace!("Successfully sent response for task {}", entry.id)
                } else {
                    tracing::error!("Failed to send response for task {}", entry.id)
                }
            }
            Stop => {
                tracing::info!("Stopping queue task");
                break 'main;
            }
        }
    }
    Ok(())
}
