//!
//! Application logic glue
//!
use futures::{future::join_all, Stream, StreamExt};
use serde::{Deserialize, Serialize};
use tce_transport::{TrbpCommands, TrbpEvents};
use tokio::spawn;
use tokio::sync::oneshot;
use topos_p2p::{Client as NetworkClient, Event as NetEvent};
use topos_tce_api::RuntimeEvent as ApiEvent;
use topos_tce_api::{RuntimeClient as ApiClient, RuntimeError};
use topos_tce_broadcast::sampler::SampleType;
use topos_tce_broadcast::DoubleEchoCommand;
use topos_tce_broadcast::{ReliableBroadcastClient, SamplerCommand};
use topos_tce_storage::events::StorageEvent;
use topos_tce_storage::StorageClient;
use tracing::{debug, error, info, trace};

/// Top-level transducer main app context & driver (alike)
///
/// Implements <...Host> traits for network and Api, listens for protocol events in events
/// (store is not active component).
///
/// In the end we shall come to design where this struct receives
/// config+data as input and runs app returning data as output
///
pub struct AppContext {
    pub trbp_cli: ReliableBroadcastClient,
    pub network_client: NetworkClient,
    pub api_client: ApiClient,
    pub pending_storage: StorageClient,
}

impl AppContext {
    /// Factory
    pub fn new(
        pending_storage: StorageClient,
        trbp_cli: ReliableBroadcastClient,
        network_client: NetworkClient,
        api_client: ApiClient,
    ) -> Self {
        Self {
            trbp_cli,
            network_client,
            api_client,
            pending_storage,
        }
    }

    /// Main processing loop
    pub async fn run(
        mut self,
        mut network_stream: impl Stream<Item = NetEvent> + Unpin,
        mut trb_stream: impl Stream<Item = Result<TrbpEvents, ()>> + Unpin,
        mut api_stream: impl Stream<Item = ApiEvent> + Unpin,
        mut storage_stream: impl Stream<Item = StorageEvent> + Unpin,
    ) {
        loop {
            tokio::select! {

                // protocol
                Some(Ok(evt)) = trb_stream.next() => {
                    self.on_protocol_event(evt).await;
                },

                // network
                Some(net_evt) = network_stream.next() => {
                    self.on_net_event(net_evt).await;
                }

                // api events
                Some(event) = api_stream.next() => {
                    self.on_api_event(event).await;
                }

                // Storage events
                Some(_event) = storage_stream.next() => {
                }
            }
        }
    }

    async fn on_api_event(&mut self, event: ApiEvent) {
        match event {
            ApiEvent::CertificateSubmitted {
                certificate,
                sender,
            } => {
                _ = self
                    .pending_storage
                    .add_pending_certificate(certificate.clone())
                    .await;
                spawn(self.trbp_cli.broadcast_new_certificate(certificate));
                _ = sender.send(Ok(()));
            }

            ApiEvent::PeerListPushed { peers, sender } => {
                let fut = self.trbp_cli.peer_changed(peers);
                spawn(async move {
                    if fut.await.is_err() {
                        _ = sender.send(Err(RuntimeError::UnableToPushPeerList));
                    } else {
                        _ = sender.send(Ok(()));
                    }
                });
            }
        }
    }

    async fn on_protocol_event(&mut self, evt: TrbpEvents) {
        debug!(
            "on_protocol_event: peer: {} event {:?}",
            &self.network_client.local_peer_id, &evt
        );
        match evt {
            TrbpEvents::CertificateDelivered { certificate } => {
                _ = self
                    .pending_storage
                    .certificate_delivered(certificate.cert_id.clone())
                    .await;
                spawn(self.api_client.dispatch_certificate(certificate));
            }

            TrbpEvents::EchoSubscribeReq { peers } => {
                // Preparing echo subscribe message
                let my_peer_id = self.network_client.local_peer_id;
                let data: Vec<u8> = NetworkMessage::from(TrbpCommands::OnEchoSubscribeReq {
                    from_peer: self.network_client.local_peer_id,
                })
                .into();
                let command_sender = self.trbp_cli.get_sampler_channel();
                // Sending echo subscribe message to send to a number of remote peers
                let future_pool = peers
                    .iter()
                    .map(|peer_id| {
                        debug!(
                            "peer_id: {} sending echo subscribe to {}",
                            &my_peer_id, &peer_id
                        );
                        self.network_client
                            .send_request::<_, NetworkMessage>(*peer_id, data.clone())
                    })
                    .collect::<Vec<_>>();

                spawn(async move {
                    // Waiting for all responses from remote peers on our echo subscription request
                    let results = join_all(future_pool).await;

                    // Process responses
                    for result in results {
                        match result {
                            Ok(message) => match message {
                                // Remote peer has replied us that he is accepting us as echo subscriber
                                NetworkMessage::Cmd(TrbpCommands::OnEchoSubscribeOk {
                                    from_peer,
                                }) => {
                                    info!("Receive response to EchoSubscribe",);
                                    let (sender, receiver) = oneshot::channel();
                                    let _ = command_sender
                                        .send(SamplerCommand::ConfirmPeer {
                                            peer: from_peer,
                                            sample_type: SampleType::EchoSubscription,
                                            sender,
                                        })
                                        .await;

                                    let _ = receiver.await.expect("Sender was dropped");
                                }
                                msg => {
                                    error!("Receive an unexpected message as a response {msg:?}")
                                }
                            },
                            Err(error) => {
                                error!("An error occurred when sending EchoSubscribe {error:?}")
                            }
                        }
                    }
                });
            }
            TrbpEvents::ReadySubscribeReq { peers } => {
                // Preparing ready subscribe message
                let my_peer_id = self.network_client.local_peer_id;
                let data: Vec<u8> = NetworkMessage::from(TrbpCommands::OnReadySubscribeReq {
                    from_peer: self.network_client.local_peer_id,
                })
                .into();
                let command_sender = self.trbp_cli.get_sampler_channel();
                // Sending ready subscribe message to send to a number of remote peers
                let future_pool = peers
                    .iter()
                    .map(|peer_id| {
                        debug!(
                            "peer_id: {} sending ready subscribe to {}",
                            &my_peer_id, &peer_id
                        );
                        self.network_client
                            .send_request::<_, NetworkMessage>(*peer_id, data.clone())
                    })
                    .collect::<Vec<_>>();

                spawn(async move {
                    // Waiting for all responses from remote peers on our ready subscription request
                    let results = join_all(future_pool).await;

                    // Process responses from remote peers
                    for result in results {
                        match result {
                            Ok(message) => match message {
                                // Remote peer has replied us that he is accepting us as ready subscriber
                                NetworkMessage::Cmd(TrbpCommands::OnReadySubscribeOk {
                                    from_peer,
                                }) => {
                                    info!("Receive response to ReadySubscribe");
                                    let (sender_ready, receiver_ready) = oneshot::channel();
                                    let _ = command_sender
                                        .send(SamplerCommand::ConfirmPeer {
                                            peer: from_peer,
                                            sample_type: SampleType::ReadySubscription,
                                            sender: sender_ready,
                                        })
                                        .await;
                                    let (sender_delivery, receiver_delivery) = oneshot::channel();
                                    let _ = command_sender
                                        .send(SamplerCommand::ConfirmPeer {
                                            peer: from_peer,
                                            sample_type: SampleType::DeliverySubscription,
                                            sender: sender_delivery,
                                        })
                                        .await;

                                    join_all(vec![receiver_ready, receiver_delivery]).await;
                                }
                                msg => {
                                    error!("Receive an unexpected message as a response {msg:?}")
                                }
                            },
                            Err(error) => {
                                error!("An error occurred when sending ReadySubscribe {error:?}")
                            }
                        }
                    }
                });
            }

            TrbpEvents::Gossip { peers, cert, .. } => {
                let cert_id = cert.cert_id.clone();
                let data: Vec<u8> = NetworkMessage::from(TrbpCommands::OnGossip {
                    cert,
                    digest: vec![],
                })
                .into();

                let future_pool = peers
                    .iter()
                    .map(|peer_id| {
                        debug!(
                            "peer_id: {} sending gossip cert id: {} to peer {:?}",
                            &self.network_client.local_peer_id, &cert_id, &peer_id
                        );
                        self.network_client
                            .send_request::<_, NetworkMessage>(*peer_id, data.clone())
                    })
                    .collect::<Vec<_>>();

                spawn(async move {
                    let _results = join_all(future_pool).await;
                });
            }

            TrbpEvents::Echo { peers, cert } => {
                let my_peer_id = self.network_client.local_peer_id;
                debug!(
                    "peer_id: {} processing on_protocol_event TrbpEvents::Echo peers {:?} cert id: {}",
                    &my_peer_id, &peers, &cert.cert_id
                );
                // Send echo message
                let data: Vec<u8> = NetworkMessage::from(TrbpCommands::OnEcho {
                    from_peer: self.network_client.local_peer_id,
                    cert,
                })
                .into();

                let future_pool = peers
                    .iter()
                    .map(|peer_id| {
                        debug!("peer_id: {} sending Echo to {}", &my_peer_id, &peer_id);
                        self.network_client
                            .send_request::<_, NetworkMessage>(*peer_id, data.clone())
                    })
                    .collect::<Vec<_>>();

                spawn(async move {
                    let _results = join_all(future_pool).await;
                });
            }

            TrbpEvents::Ready { peers, cert } => {
                let my_peer_id = self.network_client.local_peer_id;
                debug!(
                    "peer_id: {} processing TrbpEvents::Ready peers {:?} cert id: {}",
                    &my_peer_id, &peers, &cert.cert_id
                );
                let data: Vec<u8> = NetworkMessage::from(TrbpCommands::OnReady {
                    from_peer: self.network_client.local_peer_id,
                    cert,
                })
                .into();

                let future_pool = peers
                    .iter()
                    .map(|peer_id| {
                        debug!("peer_id: {} sending Ready to {}", &my_peer_id, &peer_id);
                        self.network_client
                            .send_request::<_, NetworkMessage>(*peer_id, data.clone())
                    })
                    .collect::<Vec<_>>();

                spawn(async move {
                    let _results = join_all(future_pool).await;
                });
            }
            evt => {
                debug!("Unhandled event: {:?}", evt);
            }
        }
    }

    async fn on_net_event(&mut self, evt: NetEvent) {
        trace!(
            "on_net_event: peer: {} event {:?}",
            &self.network_client.local_peer_id,
            &evt
        );
        match evt {
            NetEvent::PeersChanged { .. } => {}

            NetEvent::TransmissionOnReq {
                from: _,
                data,
                channel,
                ..
            } => {
                let my_peer = self.network_client.local_peer_id;
                let msg: NetworkMessage = data.into();
                match msg {
                    NetworkMessage::Cmd(cmd) => {
                        info!("peer_id: {} received TransmissionOnReq {:?}", &my_peer, cmd);
                        match cmd {
                            // We received echo subscription request from external peer
                            TrbpCommands::OnEchoSubscribeReq { from_peer } => {
                                debug!(
                                    "on_net_event peer {} TrbpCommands::OnEchoSubscribeReq from_peer: {}",
                                    &self.network_client.local_peer_id, &from_peer
                                );
                                self.trbp_cli
                                    .add_confirmed_peer_to_sample(
                                        SampleType::EchoSubscriber,
                                        from_peer,
                                    )
                                    .await;

                                // We are responding that we are accepting echo subscriber
                                spawn(self.network_client.respond_to_request(
                                    NetworkMessage::from(TrbpCommands::OnEchoSubscribeOk {
                                        from_peer: my_peer,
                                    }),
                                    channel,
                                ));
                            }

                            // We received ready subscription request from external peer
                            TrbpCommands::OnReadySubscribeReq { from_peer } => {
                                debug!(
                                    "peer_id {} on_net_event TrbpCommands::OnReadySubscribeReq from_peer: {}",
                                    &self.network_client.local_peer_id, &from_peer
                                );
                                self.trbp_cli
                                    .add_confirmed_peer_to_sample(
                                        SampleType::ReadySubscriber,
                                        from_peer,
                                    )
                                    .await;
                                // We are responding that we are accepting ready subscriber
                                spawn(self.network_client.respond_to_request(
                                    NetworkMessage::from(TrbpCommands::OnReadySubscribeOk {
                                        from_peer: my_peer,
                                    }),
                                    channel,
                                ));
                            }

                            TrbpCommands::OnGossip { cert, digest: _ } => {
                                debug!(
                                    "peer_id {} on_net_event TrbpCommands::OnGossip cert id: {}",
                                    &self.network_client.local_peer_id, &cert.cert_id
                                );
                                let command_sender = self.trbp_cli.get_double_echo_channel();
                                command_sender
                                    .send(DoubleEchoCommand::Broadcast { cert })
                                    .await
                                    .expect("Gossip the certificate");

                                spawn(self.network_client.respond_to_request(
                                    NetworkMessage::from(TrbpCommands::OnDoubleEchoOk {
                                        from_peer: my_peer,
                                    }),
                                    channel,
                                ));
                            }
                            TrbpCommands::OnEcho { from_peer, cert } => {
                                // We have received Echo echo message, we are responding with OnDoubleEchoOk
                                debug!(
                                    "peer_id: {} on_net_event TrbpCommands::OnEcho from peer {} cert id: {}",
                                    &self.network_client.local_peer_id, &from_peer, &cert.cert_id
                                );
                                // We have received echo message from external peer
                                let command_sender = self.trbp_cli.get_double_echo_channel();
                                command_sender
                                    .send(DoubleEchoCommand::Echo { from_peer, cert })
                                    .await
                                    .expect("Receive the Echo");
                                //We are responding with OnDoubleEchoOk to remote peer
                                spawn(self.network_client.respond_to_request(
                                    NetworkMessage::from(TrbpCommands::OnDoubleEchoOk {
                                        from_peer: my_peer,
                                    }),
                                    channel,
                                ));
                            }
                            TrbpCommands::OnReady { from_peer, cert } => {
                                // We have received Ready echo message, we are responding with OnDoubleEchoOk
                                debug!(
                                    "peer_id {} on_net_event TrbpCommands::OnReady from peer {} cert id: {}",
                                    &self.network_client.local_peer_id, &from_peer, &cert.cert_id
                                );
                                let command_sender = self.trbp_cli.get_double_echo_channel();
                                command_sender
                                    .send(DoubleEchoCommand::Ready { from_peer, cert })
                                    .await
                                    .expect("Receive the Ready");

                                spawn(self.network_client.respond_to_request(
                                    NetworkMessage::from(TrbpCommands::OnDoubleEchoOk {
                                        from_peer: my_peer,
                                    }),
                                    channel,
                                ));
                            }
                            _ => todo!(),
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Definition of networking payload.
///
/// We assume that only Commands will go through the network,
/// [Response] is used to allow reporting of logic errors to the caller.
#[derive(Debug, Clone, Serialize, Deserialize)]
enum NetworkMessage {
    Cmd(TrbpCommands),
}

// deserializer
impl From<Vec<u8>> for NetworkMessage {
    fn from(data: Vec<u8>) -> Self {
        bincode::deserialize::<NetworkMessage>(data.as_ref()).expect("msg deser")
    }
}

// serializer
impl From<NetworkMessage> for Vec<u8> {
    fn from(msg: NetworkMessage) -> Self {
        bincode::serialize::<NetworkMessage>(&msg).expect("msg ser")
    }
}

// transformer of protocol commands into network commands
impl From<TrbpCommands> for NetworkMessage {
    fn from(cmd: TrbpCommands) -> Self {
        Self::Cmd(cmd)
    }
}