use std::io;

use iceoryx2::{
    node::NodeCreationFailure,
    port::{
        LoanError, ReceiveError, SendError, publisher::PublisherCreateError,
        subscriber::SubscriberCreateError,
    },
    service::builder::publish_subscribe::PublishSubscribeOpenOrCreateError,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RoboscopeIpcError {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error("Failed to establish a new IPC node")]
    SetupNode(#[from] NodeCreationFailure),

    #[error("Failed to establish a service")]
    SetupService(#[from] PublishSubscribeOpenOrCreateError),

    #[error("Failed to begin publishing to a service")]
    PublishService(#[from] PublisherCreateError),

    #[error("Failed to begin subscribing to a service")]
    SubscribeService(#[from] SubscriberCreateError),

    #[error("Failed to receive a message")]
    Receive(#[from] ReceiveError),

    #[error("Failed to loan a data segment")]
    DataLoan(#[from] LoanError),

    #[error("Failed to send a packet")]
    SendError(#[from] SendError),
}

pub type SimResult<T> = Result<T, RoboscopeIpcError>;
