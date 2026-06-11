use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use std::pin::Pin;

pub struct QuicConnectionState {
    pub quiche_conn: Pin<Box<quiche::Connection>>,
    pub last_packet_time: std::time::Instant,
}

pub struct H3Server {
    socket: Arc<UdpSocket>,
    connections: HashMap<quiche::ConnectionId<'static>, QuicConnectionState>,
    config: quiche::Config,
}

impl H3Server {
    pub async fn new(addr: SocketAddr) -> Result<Self, Box<dyn std::error::Error>> {
        let socket = UdpSocket::bind(addr).await?;
        
        let mut config = quiche::Config::new(quiche::PROTOCOL_VERSION)?;
        
        config.set_application_protos(quiche::h3::APPLICATION_PROTOCOL)?;
        config.set_max_idle_timeout(30_000);
        config.set_max_recv_udp_payload_size(1350);
        config.set_max_send_udp_payload_size(1350);
        config.set_initial_max_data(10_000_000);
        config.set_initial_max_stream_data_bidi_local(1_000_000);
        config.set_initial_max_stream_data_bidi_remote(1_000_000);
        config.set_initial_max_streams_bidi(100);
        
        config.set_cc_algorithm(quiche::CongestionControlAlgorithm::BBR);

        Ok(Self {
            socket: Arc::new(socket),
            connections: HashMap::new(),
            config,
        })
    }

    pub async fn run_loop(mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut buf = [0u8; 65535];
        
        loop {
            tokio::select! {
                result = self.socket.recv_from(&mut buf) => {
                    let (read_len, src_addr) = match result {
                        Ok(val) => val,
                        Err(e) => {
                            log::error!("UDP read error: {:?}", e);
                            continue;
                        }
                    };

                    let packet = &mut buf[..read_len];
                    
                    let header = match quiche::Header::from_slice(packet, quiche::MAX_CONN_ID_LEN) {
                        Ok(h) => h,
                        Err(e) => {
                            log::warn!("Failed to parse packet header: {:?}", e);
                            continue;
                        }
                    };

                    let conn_id = header.dcid.clone();
                    
                    if !self.connections.contains_key(&conn_id) {
                        let local_conn = quiche::accept(
                            &conn_id,
                            None,
                            src_addr,
                            self.socket.local_addr()?,
                            &mut self.config,
                        )?;
                        
                        if let Some(sni) = local_conn.server_name() {
                            log::info!("Pre-handshake tenant context parsed from SNI: {}", sni);
                        }

                        let state = QuicConnectionState {
                            quiche_conn: Box::pin(local_conn),
                            last_packet_time: std::time::Instant::now(),
                        };
                        self.connections.insert(conn_id.clone().into_owned(), state);
                    }

                    if let Some(conn_state) = self.connections.get_mut(&conn_id) {
                        let info = quiche::RecvInfo {
                            to: self.socket.local_addr()?,
                            from: src_addr,
                        };
                        
                        match conn_state.quiche_conn.recv(packet, info) {
                            Ok(_) => {
                                conn_state.last_packet_time = std::time::Instant::now();
                                Self::flush_connection_outbox(&self.socket, &mut conn_state.quiche_conn, src_addr).await?;
                            }
                            Err(e) => {
                                log::error!("Connection data receipt failed: {:?}", e);
                            }
                        }
                    }
                }
            }
        }
    }

    async fn flush_connection_outbox(
        socket: &UdpSocket,
        conn: &mut quiche::Connection,
        dest: SocketAddr,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut out_buf = [0u8; 1350];
        
        loop {
            match conn.send(&mut out_buf) {
                Ok((write_len, _info)) => {
                    socket.send_to(&out_buf[..write_len], dest).await?;
                }
                Err(quiche::Error::Done) => {
                    break;
                }
                Err(e) => {
                    log::error!("Outbound packet flush failed: {:?}", e);
                    return Err(Box::new(e));
                }
            }
        }
        Ok(())
    }
}
