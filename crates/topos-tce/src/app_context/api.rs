use opentelemetry::trace::FutureExt as TraceFutureExt;
use std::collections::HashMap;
use tokio::spawn;
use topos_core::uci::{Certificate, SubnetId};
use topos_tce_api::RuntimeError;
use topos_tce_api::RuntimeEvent as ApiEvent;
use topos_tce_gatekeeper::GatekeeperError;
use topos_tce_storage::errors::{InternalStorageError, StorageError};
use tracing::{error, info, info_span, warn, Instrument};
use tracing_opentelemetry::OpenTelemetrySpanExt;

use crate::events::Events;
use crate::AppContext;

impl AppContext {
    pub async fn on_api_event(&mut self, event: ApiEvent) {
        match event {
            ApiEvent::CertificateSubmitted {
                certificate,
                sender,
                ctx,
            } => {
                let span = info_span!(parent: &ctx, "TCE Runtime");

                _ = self
                    .tce_cli
                    .broadcast_new_certificate(*certificate, true)
                    .with_context(span.context())
                    .instrument(span)
                    .await;

                _ = sender.send(Ok(()));
            }

            ApiEvent::PeerListPushed { peers, sender } => {
                let sampler = self.tce_cli.clone();
                let gatekeeper = self.gatekeeper.clone();
                let events = self.events.clone();
                let api = self.api_client.clone();

                spawn(async move {
                    match gatekeeper.push_peer_list(peers).await {
                        Ok(peers) => {
                            info!("Gatekeeper has detected changes on the peer list, new sample in creation");
                            if sampler.peer_changed(peers).await.is_err() {
                                _ = sender.send(Err(RuntimeError::UnableToPushPeerList));
                            } else {
                                api.set_active_sample(true).await;
                                if events.send(Events::StableSample).await.is_err() {
                                    error!("Unable to send StableSample event");
                                }
                                _ = sender.send(Ok(()));
                            }
                        }
                        Err(GatekeeperError::NoUpdate) => {
                            _ = sender.send(Ok(()));
                        }
                        Err(_) => {
                            _ = sender.send(Err(RuntimeError::UnableToPushPeerList));
                        }
                    }
                });
            }

            ApiEvent::GetSourceHead { subnet_id, sender } => {
                // Get source head certificate
                let mut result = self
                    .pending_storage
                    .get_source_head(subnet_id)
                    .await
                    .map_err(|e| match e {
                        StorageError::InternalStorage(internal) => {
                            if let InternalStorageError::MissingHeadForSubnet(subnet_id) = internal
                            {
                                RuntimeError::UnknownSubnet(subnet_id)
                            } else {
                                RuntimeError::UnableToGetSourceHead(subnet_id, internal.to_string())
                            }
                        }
                        e => RuntimeError::UnableToGetSourceHead(subnet_id, e.to_string()),
                    });

                // TODO: Initial genesis certificate eventually will be fetched from the topos subnet
                // Currently, for subnet starting from scratch there are no certificates in the database
                // So for MissingHeadForSubnet error we will return some default dummy certificate
                if let Err(RuntimeError::UnknownSubnet(subnet_id)) = result {
                    warn!("Returning dummy certificate as head certificate, to be fixed...");
                    result = Ok((
                        0,
                        topos_core::uci::Certificate {
                            prev_id: AppContext::DUMMY_INITIAL_CERTIFICATE_ID,
                            source_subnet_id: subnet_id,
                            state_root: Default::default(),
                            tx_root_hash: Default::default(),
                            target_subnets: vec![],
                            verifier: 0,
                            id: AppContext::DUMMY_INITIAL_CERTIFICATE_ID,
                            proof: Default::default(),
                            signature: Default::default(),
                        },
                    ));
                };

                _ = sender.send(result);
            }

            ApiEvent::GetLastPendingCertificates {
                mut subnet_ids,
                sender,
            } => {
                let mut last_pending_certificates: HashMap<SubnetId, Option<Certificate>> =
                    subnet_ids
                        .iter()
                        .map(|subnet_id| (*subnet_id, None))
                        .collect();

                if let Ok(pending_certificates) =
                    self.pending_storage.get_pending_certificates().await
                {
                    // Iterate through pending certificates and determine last one for every subnet
                    // Last certificate in the subnet should be one with the highest index
                    for (_pending_certificate_id, cert) in pending_certificates.into_iter().rev() {
                        if let Some(subnet_id) = subnet_ids.take(&cert.source_subnet_id) {
                            *last_pending_certificates.entry(subnet_id).or_insert(None) =
                                Some(cert);
                        }
                        if subnet_ids.is_empty() {
                            break;
                        }
                    }
                }

                // Add None pending certificate for any other requested subnet_id
                subnet_ids.iter().for_each(|subnet_id| {
                    last_pending_certificates.insert(*subnet_id, None);
                });

                _ = sender.send(Ok(last_pending_certificates));
            }
        }
    }
}