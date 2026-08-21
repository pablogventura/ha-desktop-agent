mod discovery;

pub use discovery::{
    availability_topic, command_filter, discovery_payload, discovery_topic, parse_command_entity,
    resolve_device_id, state_topic, warn_if_world_readable,
};

use crate::action::IncomingCommand;
use crate::config::Config;
use rumqttc::{AsyncClient, Event, Incoming, LastWill, MqttOptions, QoS, Transport};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{info, warn};

pub struct MqttTransport {
    client: AsyncClient,
    commands: mpsc::Receiver<IncomingCommand>,
    connected: tokio::sync::watch::Receiver<bool>,
}

impl MqttTransport {
    pub fn start(config: &Config, device_id: &str) -> anyhow::Result<Self> {
        let mut options = MqttOptions::new(
            format!("ha-desktop-agent-{device_id}"),
            &config.mqtt.host,
            config.mqtt.port,
        );
        options.set_keep_alive(Duration::from_secs(30));
        options.set_clean_session(false);
        options.set_max_packet_size(256 * 1024, 256 * 1024);
        if let Some(user) = &config.mqtt.username {
            options.set_credentials(user, config.mqtt.password.clone().unwrap_or_default());
        }
        let availability = availability_topic(config, device_id);
        options.set_last_will(LastWill::new(
            availability,
            "offline",
            QoS::AtLeastOnce,
            true,
        ));
        if config.mqtt.tls {
            if config.mqtt.insecure_skip_verify {
                anyhow::bail!(
                    "mqtt.insecure_skip_verify is not supported; provide a valid CA or system trust"
                );
            }
            if let Some(ca) = &config.mqtt.ca {
                let pem = std::fs::read(ca)?;
                options.set_transport(Transport::tls_with_config(
                    rumqttc::TlsConfiguration::Simple {
                        ca: pem,
                        alpn: None,
                        client_auth: None,
                    },
                ));
            } else {
                options.set_transport(Transport::tls_with_default_config());
            }
        }

        let (client, mut eventloop) = AsyncClient::new(options, 64);
        let (cmd_tx, cmd_rx) = mpsc::channel(32);
        let (conn_tx, conn_rx) = tokio::sync::watch::channel(false);
        let prefix = config.mqtt.topic_prefix.clone();
        let device = device_id.to_string();
        let availability = availability_topic(config, device_id);
        let command_topic = command_filter(config, device_id);
        let online_client = client.clone();

        tokio::spawn(async move {
            loop {
                match eventloop.poll().await {
                    Ok(Event::Incoming(Incoming::ConnAck(_))) => {
                        info!("connected to MQTT broker");
                        let _ = conn_tx.send(true);
                        // After a broker drop, LWT may have retained `offline`. Republish
                        // availability and re-subscribe on every connect (including reconnects).
                        let client = online_client.clone();
                        let availability = availability.clone();
                        let command_topic = command_topic.clone();
                        tokio::spawn(async move {
                            if let Err(err) = client
                                .subscribe(command_topic, QoS::AtLeastOnce)
                                .await
                            {
                                warn!("MQTT resubscribe failed: {err}");
                            }
                            if let Err(err) = client
                                .publish(availability, QoS::AtLeastOnce, true, "online")
                                .await
                            {
                                warn!("MQTT availability online publish failed: {err}");
                            }
                        });
                    }
                    Ok(Event::Incoming(Incoming::Publish(publish))) => {
                        if let Some(entity_id) =
                            parse_command_entity(&publish.topic, &device, &prefix)
                        {
                            let payload = String::from_utf8_lossy(&publish.payload);
                            match IncomingCommand::parse(&entity_id, &payload) {
                                Some(command) => {
                                    if cmd_tx.send(command).await.is_err() {
                                        break;
                                    }
                                }
                                None => warn!(
                                    topic = %publish.topic,
                                    payload = %payload,
                                    "ignored MQTT command payload"
                                ),
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(err) => {
                        warn!("MQTT event loop error: {err}");
                        let _ = conn_tx.send(false);
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                }
            }
        });

        Ok(Self {
            client,
            commands: cmd_rx,
            connected: conn_rx,
        })
    }

    pub async fn wait_connected(&mut self) {
        let _ = self.connected.wait_for(|c| *c).await;
    }

    pub async fn publish_discovery(&self, config: &Config, device_id: &str) -> anyhow::Result<()> {
        let topic = discovery_topic(config, device_id);
        let payload = discovery_payload(config, device_id).to_string();
        self.client
            .publish(topic, QoS::AtLeastOnce, true, payload)
            .await?;
        Ok(())
    }

    pub async fn publish_online(&self, config: &Config, device_id: &str) -> anyhow::Result<()> {
        self.client
            .publish(
                availability_topic(config, device_id),
                QoS::AtLeastOnce,
                true,
                "online",
            )
            .await?;
        Ok(())
    }

    pub async fn subscribe_commands(&self, config: &Config, device_id: &str) -> anyhow::Result<()> {
        self.client
            .subscribe(command_filter(config, device_id), QoS::AtLeastOnce)
            .await?;
        Ok(())
    }

    pub async fn publish_state(
        &self,
        config: &Config,
        device_id: &str,
        payload: String,
    ) -> anyhow::Result<()> {
        self.client
            .publish(
                state_topic(config, device_id),
                QoS::AtLeastOnce,
                true,
                payload,
            )
            .await?;
        Ok(())
    }

    pub async fn recv_command(&mut self) -> Option<IncomingCommand> {
        self.commands.recv().await
    }

    pub fn try_recv_command(&mut self) -> Option<IncomingCommand> {
        self.commands.try_recv().ok()
    }
}
