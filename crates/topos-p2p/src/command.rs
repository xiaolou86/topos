use std::fmt::Display;

use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use crate::{
    behaviour::grpc::connection::OutboundConnection,
    error::{CommandExecutionError, P2PError},
};

#[derive(Debug)]
pub enum Command {
    /// Executed when the node is starting
    StartListening {
        peer_addr: Multiaddr,
        sender: oneshot::Sender<Result<(), P2PError>>,
    },

    /// Command to initiate a dial with another peer.
    /// If we already dialled the peer, an error is returned
    /// If the peer that we want to dial is self, an error is returned
    /// If we can't initiate a dial with the peer, an error is returned
    Dial {
        peer_id: PeerId,
        peer_addr: Multiaddr,
        sender: oneshot::Sender<Result<(), P2PError>>,
    },

    /// Command to ask for the current connected peer id list
    ConnectedPeers {
        sender: oneshot::Sender<Result<Vec<PeerId>, P2PError>>,
    },

    /// Disconnect the node
    Disconnect {
        sender: oneshot::Sender<Result<(), P2PError>>,
    },

    /// Try to discover a peer based on its PeerId
    Discover {
        to: PeerId,
        sender: oneshot::Sender<Result<Vec<Multiaddr>, CommandExecutionError>>,
    },

    Gossip {
        topic: &'static str,
        data: Vec<u8>,
    },

    /// Ask for the creation of a new proxy connection for a gRPC query.
    /// The response will be sent to the sender of the command once the connection is established.
    /// The response will be a `OutboundConnection` that can be used to create a gRPC client.
    /// A connection is established if needed with the peer.
    NewProxiedQuery {
        protocol: &'static str,
        peer: PeerId,
        id: uuid::Uuid,
        response: oneshot::Sender<OutboundConnection>,
    },
}

impl Display for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Command::StartListening { .. } => write!(f, "StartListening"),
            Command::Dial { peer_id, .. } => write!(f, "Dial({peer_id})"),
            Command::ConnectedPeers { .. } => write!(f, "ConnectedPeers"),
            Command::Disconnect { .. } => write!(f, "Disconnect"),
            Command::Gossip { .. } => write!(f, "GossipMessage"),
            Command::NewProxiedQuery { .. } => write!(f, "NewProxiedQuery"),
            Command::Discover { to, .. } => write!(f, "Discover(to: {to})"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotReadyMessage {}
