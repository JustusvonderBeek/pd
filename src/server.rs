
use std::collections::{HashSet, HashMap};
use std::net::{UdpSocket, SocketAddr};
use pretty_hex::*;
use std::io::Write;
use std::fs::OpenOptions;
use std::fs;
use rand::Rng;
use sha2::{Sha256, Digest};
use std::convert::TryInto;
use crate::cmdline_handler::Options;
use crate::packets::*;

const DEBUG_PACKET_SIZE : usize = 100;

enum ConnectionState {
    Setup,
    Transfer,
    Retransmission,
    Error,
}

// TODO: Add the values we need for the operation
pub struct TBDServer {
    options : Options,
    conn_ids : HashSet<u32>,
    states : HashMap<u32, ConnectionStore>,
}

struct ConnectionStore {
    state : ConnectionState,
    block_id : u32,
    flow_window : u32,
    file_size : u64,
    send : u64,
    endpoint : SocketAddr,
}

impl TBDServer {
    pub fn create(opt: Options) -> TBDServer {
        TBDServer {
            options : opt,
            conn_ids : HashSet::new(),
            states : HashMap::new(),
        }
    }

    pub fn start(&mut self) -> std::io::Result<()> {
        self.server()
    }

    fn server(&mut self) -> std::io::Result<()> {
        
        info!("Starting the server...");
    
        // Bind to given hostname
        let mut hostname = String::from(&self.options.hostname);
        hostname.push_str(":");
        hostname.push_str(&self.options.server_port.to_string());
        let sock = UdpSocket::bind(hostname).unwrap();
        info!("Server is listening on {}", sock.local_addr().unwrap());
    
        loop {
            // 1. Reading a new packet (because this is not threaded block here)

            let (packet, addr) = self.next_packet(&sock).unwrap();

            // 2. Check for a new client (according to protocol we always get a new request on resumption)
    
            if check_packet_type(&packet, PacketType::Request) {
                // New client
                let packet = match RequestPacket::deserialize(&packet) {
                    Ok(p) => p,
                    Err(e) => {
                        error!("Failed to deserialize request packet: {}", e);
                        // We cannot do more than continue and wait for the client to send the next packet; basically ignore the packet
                        continue;
                    },
                };

                // Check for the file status (available, readable)

                let filename = String::from(packet.file_name);
                // debug!("Filename: {}", pretty_hex(&filename));

                // Removing whitespace and 0 bytes from the transfer
                let filename = filename.trim().trim_matches(char::from(0));
                debug!("Filename: {}", pretty_hex(&filename));
                let file = match fs::read(&filename) {
                    Ok(f) => f,
                    Err(e) => {
                        warn!("Failed to read in the file {}\n{}", filename, e);
                        self.send_error(&sock, &addr, ErrorTypes::FileUnavailable);
                        continue;
                    },
                };
                let filesize = file.len() as u64;

                // Compute the checksum
                let mut hasher = Sha256::new();
                hasher.update(&file);
                let hash = hasher.finalize();
                debug!("Generated hash: {}", pretty_hex(&hash));
                let filehash : [u8;32] = match hash.as_slice().try_into() {
                    Ok(h) => h,
                    Err(e) => {
                        warn!("Failed to convert hash: {}", e);
                        self.send_error(&sock, &addr, ErrorTypes::Abort);
                        continue;
                    }
                };

                // Create the new state
                let connection_id = self.generate_conn_id();
                self.create_state(connection_id, filesize, addr);
                
                // Construct the answer packet
                let resp = ResponsePacket::serialize(connection_id, 0, 0, filehash, filesize);
                match sock.send_to(&resp, addr) {
                    Ok(size) => debug!("Sent {} bytes to {}", size, addr),
                    Err(_) => {
                        warn!("Failed to transfer data to {}", addr);
                        self.remove_state(connection_id);
                    } 
                }

                // Start transfer
                let data = DataPacket::serialize(connection_id, 0, 1, 0, file);
                match sock.send_to(&data, addr) {
                    Ok(size) => debug!("Sent {} bytes to {}", size, addr),
                    Err(_) => {
                        warn!("Failed to transfer data to {}", addr);
                        self.remove_state(connection_id);
                    } 
                }

            } else {
                // Existing transfer
                if check_packet_type(&packet, PacketType::Error) {
                    let err = match ErrorPacket::deserialize(&packet) {
                        Ok(p) => p,
                        Err(e) => {
                            warn!("Failed to deserialize error packet: {}", e);
                            continue;
                        },
                    };
                    warn!("Got an error from {}", addr);
                    warn!("Error Code: {}", err.error_code);
                    self.remove_state(err.connection_id);
                    continue;
                }

                let connection_id : u32 = match get_connection_id(&packet) {
                    Ok(id) => id,
                    Err(_) => {
                        warn!("Failed to parse connection ID!");
                        continue;
                    },
                };

                // handle_retransmission(packet);

                // handle_transmission(packet);
                if check_packet_type(&packet, PacketType::Ack) {
                    let ack = AckPacket::deserialize(&packet).unwrap();
                    // TODO: Check for the state, if the transfer is complete and so on
                    let state = self.states.get(&ack.connection_id);
                    // if state == ConnectionState::
                }

                error!("Expected an acknowledgment or error but got something else!\n{}", pretty_hex(&packet));
            }
        } 
        
    }

    fn next_packet(&mut self, sock : &UdpSocket) -> Result<(Vec<u8>, SocketAddr), ()> {
        let mut buf : [u8; DEBUG_PACKET_SIZE] = [0; DEBUG_PACKET_SIZE];
        debug!("Waiting for new incoming packet");
        let (len, addr) = match sock.recv_from(&mut buf) {
            Ok(l) => l,
            Err(e) => {
                warn!("Failed to receive packet {:?}", e); 
                return Err(());
            }
        };
        debug!("Received {} bytes from {}", len, addr);
        debug!("Data:\n{}", pretty_hex(&buf));

        Ok((buf.to_vec(), addr))
    }

    fn generate_conn_id(&mut self) -> u32 {
        let mut rnd = rand::thread_rng();
        loop {
            let val = rnd.gen_range(0..2^24-1);
            if !self.conn_ids.contains(&val) {
                return val;
            }
        }
    }

    fn remove_state(&mut self, connection_id : u32) {
        self.conn_ids.remove(&connection_id);
        self.states.remove(&connection_id);
    }

    fn create_state(&mut self, connection_id : u32, size : u64, remote : SocketAddr) {
        self.conn_ids.insert(connection_id);
        let state = ConnectionStore {
            state : ConnectionState::Setup,
            block_id : 0,
            flow_window : 8,
            file_size : size,
            send : 0,
            endpoint : remote,
        };
        self.states.insert(connection_id, state);
    }

    fn send_error(&self, sock : &UdpSocket, addr : &SocketAddr, e : ErrorTypes) {
        let val = match e {
            ErrorTypes::FileUnavailable => 0x01,
            ErrorTypes::ConnectionRefused => 0x02,
            ErrorTypes::FileModified => 0x03,
            ErrorTypes::Abort => 0x04,
        };
        let err = ErrorPacket::serialize(0, 0, 0x40, val);
        match sock.send_to(&err, addr) {
            Ok(s) => debug!("Sent {} bytes of error message", s),
            Err(e) => warn!("Failed to send error message: {}", e),
        };
    }
}
